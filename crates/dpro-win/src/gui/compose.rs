//! The composition presenter (TD-0009 §5.2): the canvas uploaded into a `DirectComposition` swap
//! chain, which DWM composes over the window's system backdrop (Mica), so the client area can be
//! translucent. The GDI presenter can't do that: a GDI window is opaque.
//!
//! - **Loaded at run time.** Base `WinPE` has no d3d11, dxgi or dcomp (S-01, run 41598796130), and a
//!   static import would stop the executable from starting there. They come from System32 only
//!   ([`crate::dll`]).
//! - **Which device.** DWM composes on adapter 0, which must be a hardware one. A GPU-less
//!   machine's Microsoft Basic Render Driver is a software adapter, and WARP a software renderer, so
//!   [`Compose::Hardware`] makes no device there and the window presents with GDI (decision #30j).
//!   [`Compose::Any`] (`--materials glass`) falls back to WARP.
//! - **Presenting damage.** The swap chain has two buffers, so the back buffer holds the frame
//!   before last. Each present uploads this frame's damage and what that buffer lacks (normally the
//!   last frame's damage; [`Staleness`]), so the buffer equals the canvas without copying a buffer,
//!   and names only this frame's damage to `Present1`, as its contract asks (TD-0009 §3). New
//!   buffers (made, resized) are blank, and their first present is whole.
//! - **Device loss.** A present or resize that fails means the device is gone (removed, reset, a
//!   driver update). The window rebuilds the presenter: on hardware once, and on WARP after a
//!   second loss within [`LOSS_WINDOW`] or when no hardware device can be made (§5.2).

use std::{
    ffi::c_void,
    mem::transmute,
    ptr::{null, null_mut},
    sync::OnceLock,
    time::{Duration, Instant},
};

use windows::{
    Win32::{
        Foundation::{HMODULE, HWND, RECT},
        Graphics::{
            Direct3D::{
                D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_UNKNOWN, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL,
            },
            Direct3D11::{
                D3D11_BOX, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_FLAG,
                D3D11_SDK_VERSION, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
            },
            DirectComposition::{IDCompositionDevice, IDCompositionTarget, IDCompositionVisual},
            Dxgi::{
                Common::{
                    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM,
                    DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC,
                },
                DXGI_ADAPTER_DESC1, DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_PRESENT,
                DXGI_PRESENT_PARAMETERS, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
                DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIAdapter1, IDXGIDevice, IDXGIFactory1,
                IDXGIFactory2, IDXGISwapChain1,
            },
        },
    },
    core::{BOOL, GUID, HRESULT, Interface},
};

use super::{RectPx, SizePx};
use crate::dll::{self, Proc};

/// A second device loss within this long of the first moves the presenter to WARP for good.
pub(super) const LOSS_WINDOW: Duration = Duration::from_secs(60);

/// The PCI vendor id of Microsoft's own adapters, and the device id of the Basic Render Driver
/// (and WARP): the adapter of a machine without a display driver.
const MICROSOFT: u32 = 0x1414;
const BASIC_RENDER: u32 = 0x8C;

/// How a window may present (TD-0009 §5.2). The caller decides from `WinPE`, the Windows build and
/// the user's preference; the window then tries what this allows, cheapest checks first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Compose {
    /// GDI only.
    #[default]
    Never,
    /// `DirectComposition` on a hardware device; GDI where there's none (decision #30j).
    Hardware,
    /// `DirectComposition` on a hardware device, else on WARP (`--materials glass`, for G1).
    Any,
}

/// The device a presenter runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Device {
    Hardware,
    Warp,
}

/// What a lost device should be rebuilt on (§5.2): hardware once, WARP after a second loss within
/// [`LOSS_WINDOW`] of the last.
pub(super) fn after_loss(last: Option<Instant>, now: Instant) -> Device {
    match last {
        Some(t) if now.saturating_duration_since(t) < LOSS_WINDOW => Device::Warp,
        _ => Device::Hardware,
    }
}

/// Whether DWM's adapter can't draw in hardware: it says it's a software one, or it's the Basic
/// Render Driver, whichever the driver reports.
pub(super) fn is_software(desc: &DXGI_ADAPTER_DESC1) -> bool {
    desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0.cast_unsigned() != 0
        || (desc.VendorId == MICROSOFT && desc.DeviceId == BASIC_RENDER)
}

type CreateDevice = unsafe extern "system" fn(
    *mut c_void,
    D3D_DRIVER_TYPE,
    HMODULE,
    D3D11_CREATE_DEVICE_FLAG,
    *const D3D_FEATURE_LEVEL,
    u32,
    u32,
    *mut *mut c_void,
    *mut D3D_FEATURE_LEVEL,
    *mut *mut c_void,
) -> HRESULT;
type CreateFactory = unsafe extern "system" fn(*const GUID, *mut *mut c_void) -> HRESULT;
type CreateComposition =
    unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT;

/// The three entry points, resolved once.
#[derive(Clone, Copy)]
struct Exports {
    device: CreateDevice,
    factory: CreateFactory,
    composition: CreateComposition,
}

/// `None` where any of the three DLLs or exports is missing (`WinPE`).
fn exports() -> Option<Exports> {
    static EXPORTS: OnceLock<Option<Exports>> = OnceLock::new();
    *EXPORTS.get_or_init(|| {
        let d3d11 = dll::system("d3d11.dll").ok()?;
        let dxgi = dll::system("dxgi.dll").ok()?;
        let dcomp = dll::system("dcomp.dll").ok()?;
        let device = dll::export(d3d11, c"D3D11CreateDevice").ok()?;
        let factory = dll::export(dxgi, c"CreateDXGIFactory1").ok()?;
        let composition = dll::export(dcomp, c"DCompositionCreateDevice").ok()?;
        // SAFETY: each export has the signature its header gives it (d3d11.h, dxgi.h, dcomp.h).
        Some(unsafe {
            Exports {
                device: transmute::<Proc, CreateDevice>(device),
                factory: transmute::<Proc, CreateFactory>(factory),
                composition: transmute::<Proc, CreateComposition>(composition),
            }
        })
    })
}

/// DXGI's adapter 0, the one DWM composes on, and its description.
fn adapter0(e: Exports) -> Option<(IDXGIAdapter1, DXGI_ADAPTER_DESC1)> {
    let mut raw = null_mut();
    // SAFETY: the interface id we ask for, and an out-pointer to a local.
    unsafe { (e.factory)(&IDXGIFactory1::IID, &raw mut raw) }
        .ok()
        .ok()?;
    // SAFETY: an owned reference of the requested interface.
    let factory = unsafe { IDXGIFactory1::from_raw(raw) };
    // SAFETY: plain COM calls on a live factory and adapter.
    unsafe {
        let adapter = factory.EnumAdapters1(0).ok()?;
        let desc = adapter.GetDesc1().ok()?;
        Some((adapter, desc))
    }
}

/// What DWM's adapter is, for G1's record (TD-0009 §3's verification): its name, PCI ids, and
/// whether this presenter takes it for a software one.
#[cfg(test)]
fn adapter_summary() -> Option<String> {
    let (_, desc) = adapter0(exports()?)?;
    let len = desc
        .Description
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(desc.Description.len());
    Some(format!(
        "{} (vendor {:#06x}, device {:#06x}, flags {:#x}, software {})",
        String::from_utf16_lossy(&desc.Description[..len]),
        desc.VendorId,
        desc.DeviceId,
        desc.Flags,
        is_software(&desc)
    ))
}

/// A Direct3D 11 device with BGRA support, and a `DirectComposition` device over it.
pub(super) struct Gpu {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    factory: IDXGIFactory2,
    dcomp: IDCompositionDevice,
    warp: bool,
}

impl Gpu {
    /// The devices `compose` allows, hardware first; `None` if it allows none (TD-0009 §5.2's
    /// steps 3–5; the caller has taken 1 and 2).
    pub(super) fn open(compose: Compose) -> Option<Self> {
        match compose {
            Compose::Never => None,
            Compose::Hardware => Self::new(Device::Hardware),
            Compose::Any => Self::new(Device::Hardware).or_else(|| Self::new(Device::Warp)),
        }
    }

    /// The devices on `device`; `None` if a DLL is missing, adapter 0 is a software one (for
    /// hardware), or a device can't be made.
    pub(super) fn new(device: Device) -> Option<Self> {
        let e = exports()?;
        let (adapter, driver) = match device {
            Device::Hardware => {
                let (adapter, desc) = adapter0(e)?;
                if is_software(&desc) {
                    return None;
                }
                (Some(adapter), D3D_DRIVER_TYPE_UNKNOWN)
            }
            Device::Warp => (None, D3D_DRIVER_TYPE_WARP),
        };
        let (mut raw_device, mut raw_context) = (null_mut(), null_mut());
        // SAFETY: a live adapter (or none, with WARP's driver type), default feature levels, and
        // out-pointers to locals.
        unsafe {
            (e.device)(
                adapter.as_ref().map_or(null_mut(), Interface::as_raw),
                driver,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                null(),
                0,
                D3D11_SDK_VERSION,
                &raw mut raw_device,
                null_mut(),
                &raw mut raw_context,
            )
        }
        .ok()
        .ok()?;
        if raw_device.is_null() || raw_context.is_null() {
            return None;
        }
        // SAFETY: both are owned references the call just returned.
        let (device_3d, context) = unsafe {
            (
                ID3D11Device::from_raw(raw_device),
                ID3D11DeviceContext::from_raw(raw_context),
            )
        };
        let dxgi: IDXGIDevice = device_3d.cast().ok()?;
        // SAFETY: plain COM calls on live objects: the factory that made the device's adapter.
        let factory: IDXGIFactory2 = unsafe { dxgi.GetAdapter().ok()?.GetParent().ok()? };
        let mut raw = null_mut();
        // SAFETY: a live DXGI device, the interface id we ask for, and an out-pointer to a local.
        unsafe { (e.composition)(dxgi.as_raw(), &IDCompositionDevice::IID, &raw mut raw) }
            .ok()
            .ok()?;
        // SAFETY: an owned reference of the requested interface.
        let dcomp = unsafe { IDCompositionDevice::from_raw(raw) };
        Some(Self {
            device: device_3d,
            context,
            factory,
            dcomp,
            warp: device == Device::Warp,
        })
    }

    /// It runs on WARP.
    pub(super) fn warp(&self) -> bool {
        self.warp
    }

    /// A composition swap chain of `format` at `size` with premultiplied alpha: the format checks
    /// G1 records (§3's verification), and the presenter's own.
    pub(super) fn swap_chain(
        &self,
        size: SizePx,
        format: DXGI_FORMAT,
    ) -> windows::core::Result<IDXGISwapChain1> {
        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: size.w.max(1),
            Height: size.h.max(1),
            Format: format,
            Stereo: false.into(),
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            // The only scaling a composition swap chain takes.
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
            AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
            Flags: 0,
        };
        // SAFETY: live devices and a valid descriptor that outlives the call.
        unsafe {
            self.factory
                .CreateSwapChainForComposition(&self.device, &raw const desc, None)
        }
    }
}

/// A device was lost (or a present failed for another reason): the presenter must be rebuilt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Lost;

impl From<windows::core::Error> for Lost {
    fn from(_: windows::core::Error) -> Self {
        Self
    }
}

/// The most rectangles a present names; more merge into their bounding box.
const MAX_RECTS: usize = 16;

/// Up to [`MAX_RECTS`] rectangles, without allocating: a frame's damage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Rects {
    r: [RectPx; MAX_RECTS],
    n: usize,
}

impl Rects {
    /// One rectangle over all of `size`.
    pub(super) fn whole(size: SizePx) -> Self {
        let mut all = Self::default();
        all.push(bounds(size));
        all
    }

    /// Adds `r`, unless it's empty or already here; past [`MAX_RECTS`] everything becomes one
    /// bounding box.
    pub(super) fn push(&mut self, r: RectPx) {
        if r.x1 <= r.x0 || r.y1 <= r.y0 || self.as_slice().contains(&r) {
            return;
        }
        if self.n == MAX_RECTS {
            let b = self.as_slice().iter().fold(r, |b, q| RectPx {
                x0: b.x0.min(q.x0),
                y0: b.y0.min(q.y0),
                x1: b.x1.max(q.x1),
                y1: b.y1.max(q.y1),
            });
            *self = Self::default();
            self.r[0] = b;
            self.n = 1;
            return;
        }
        self.r[self.n] = r;
        self.n += 1;
    }

    pub(super) fn as_slice(&self) -> &[RectPx] {
        &self.r[..self.n]
    }

    pub(super) fn is_empty(&self) -> bool {
        self.n == 0
    }

    pub(super) fn clear(&mut self) {
        self.n = 0;
    }

    /// These and `other`'s.
    fn union(&self, other: &Self) -> Self {
        let mut all = *self;
        for r in other.as_slice() {
            all.push(*r);
        }
        all
    }

    /// Each rectangle clipped to `size`; the empty ones dropped.
    fn clipped(&self, size: SizePx) -> Self {
        let b = bounds(size);
        let mut out = Self::default();
        for r in self.as_slice() {
            out.push(RectPx {
                x0: r.x0.max(b.x0),
                y0: r.y0.max(b.y0),
                x1: r.x1.min(b.x1),
                y1: r.y1.min(b.y1),
            });
        }
        out
    }

    /// The pixels the rectangles cover, overlaps counted twice.
    fn area(&self) -> u64 {
        self.as_slice()
            .iter()
            .map(|r| u64::from(r.x0.abs_diff(r.x1)) * u64::from(r.y0.abs_diff(r.y1)))
            .sum()
    }
}

fn bounds(size: SizePx) -> RectPx {
    RectPx {
        x0: 0,
        y0: 0,
        x1: i32::try_from(size.w).unwrap_or(i32::MAX),
        y1: i32::try_from(size.h).unwrap_or(i32::MAX),
    }
}

/// What each of the swap chain's two buffers lacks of the canvas, so a present uploads only that.
///
/// With two flip buffers, the back buffer holds the frame before last. After a present it holds
/// everything, and the other buffer, the next back one, lacks what it lacked and this frame's
/// damage. New buffers lack everything, and the first present names no rectangles, which means all
/// of the buffer, as `Present1` requires.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Staleness {
    size: SizePx,
    /// What the next back buffer lacks, besides new damage.
    back: Rects,
    /// What the buffer on screen lacks: what the back buffer will lack after the next present.
    front: Rects,
    /// The screen doesn't show the canvas: the buffers are new.
    pending: bool,
}

/// One present: what to upload, and what changed since the frame on screen (`None`: everything).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Plan {
    upload: Rects,
    dirty: Option<Rects>,
}

impl Staleness {
    /// New buffers of `size`.
    pub(super) fn new(size: SizePx) -> Self {
        Self {
            size,
            back: Rects::whole(size),
            front: Rects::whole(size),
            pending: true,
        }
    }

    /// The present for `damage`; `None` if there's none to make.
    pub(super) fn plan(&self, damage: &Rects) -> Option<Plan> {
        let damage = damage.clipped(self.size);
        if damage.is_empty() && !self.pending {
            return None;
        }
        let all = u64::from(self.size.w) * u64::from(self.size.h);
        let mut upload = self.back.union(&damage);
        // Covering about everything, one upload is cheaper than many overlapping ones.
        if upload.area() >= all {
            upload = Rects::whole(self.size);
        }
        let dirty = (!self.pending && damage.area() < all).then_some(damage);
        Some(Plan { upload, dirty })
    }

    /// A present of `damage` was made.
    pub(super) fn presented(&mut self, damage: &Rects) {
        self.back = self.front.union(&damage.clipped(self.size));
        self.front = Rects::default();
        self.pending = false;
    }
}

/// A window's composition presenter: the devices, the swap chain, and the visual that shows it.
pub(super) struct Composer {
    gpu: Gpu,
    swap: IDXGISwapChain1,
    /// Binds the visual to the window; releasing it unbinds it, so it lives as long as the swap
    /// chain shows there.
    _target: IDCompositionTarget,
    _visual: IDCompositionVisual,
    stale: Staleness,
}

impl Composer {
    /// Shows a swap chain of `size` in `hwnd`, which has `WS_EX_NOREDIRECTIONBITMAP` and no
    /// composition target; `None` if that fails.
    pub(super) fn new(
        gpu: Gpu,
        hwnd: windows_sys::Win32::Foundation::HWND,
        size: SizePx,
    ) -> Option<Self> {
        let size = at_least_one(size);
        let swap = gpu.swap_chain(size, DXGI_FORMAT_B8G8R8A8_UNORM).ok()?;
        // SAFETY: live devices, and a live window of this thread.
        let (target, visual) = unsafe {
            gpu.dcomp
                .CreateTargetForHwnd(HWND(hwnd), true)
                .and_then(|target| {
                    let visual = gpu.dcomp.CreateVisual()?;
                    visual.SetContent(&swap)?;
                    target.SetRoot(&visual)?;
                    gpu.dcomp.Commit()?;
                    Ok((target, visual))
                })
        }
        .ok()?;
        Some(Self {
            gpu,
            swap,
            _target: target,
            _visual: visual,
            stale: Staleness::new(size),
        })
    }

    /// The devices of [`Device`] `device`, shown in `hwnd`.
    pub(super) fn open(
        hwnd: windows_sys::Win32::Foundation::HWND,
        size: SizePx,
        device: Device,
    ) -> Option<Self> {
        Self::new(Gpu::new(device)?, hwnd, size)
    }

    /// It runs on WARP.
    pub(super) fn warp(&self) -> bool {
        self.gpu.warp
    }

    /// Whether its `DirectComposition` device is still valid: DWM restarting invalidates it.
    pub(super) fn healthy(&self) -> bool {
        // SAFETY: a plain COM call on a live device.
        unsafe { self.gpu.dcomp.CheckDeviceState() }.is_ok_and(BOOL::as_bool)
    }

    /// Resizes the buffers to the client area; their pixels are gone, so the next present is whole.
    ///
    /// # Errors
    /// The device was lost.
    pub(super) fn resize(&mut self, size: SizePx) -> Result<(), Lost> {
        let size = at_least_one(size);
        if size == self.stale.size {
            return Ok(());
        }
        // SAFETY: no reference to a buffer is held between presents.
        unsafe {
            self.swap.ResizeBuffers(
                0,
                size.w,
                size.h,
                DXGI_FORMAT_UNKNOWN,
                DXGI_SWAP_CHAIN_FLAG(0),
            )?;
        }
        self.stale = Staleness::new(size);
        Ok(())
    }

    /// Whether a present is due even without new damage: the buffers are new, and the canvas
    /// holds the frame.
    pub(super) fn due(&self) -> bool {
        self.stale.pending
    }

    /// Uploads `damage`, and what the back buffer lacks, from `pixels` (premultiplied BGRA rows of
    /// the swap chain's width), then presents them. `pixels` of another size present nothing, and
    /// the next present of the right size is whole.
    ///
    /// # Errors
    /// The device was lost.
    pub(super) fn present(&mut self, pixels: &[u32], damage: &Rects) -> Result<(), Lost> {
        let SizePx { w, h } = self.stale.size;
        if usize::try_from(u64::from(w) * u64::from(h)).ok() != Some(pixels.len()) {
            self.stale = Staleness::new(self.stale.size);
            return Ok(());
        }
        let Some(plan) = self.stale.plan(damage) else {
            return Ok(());
        };
        // SAFETY: a plain COM call; the texture is released before the present.
        let back: ID3D11Texture2D = unsafe { self.swap.GetBuffer(0)? };
        let stride = usize::try_from(w).unwrap_or(usize::MAX);
        for r in plan.upload.as_slice() {
            // Clipped to the buffer, so none is negative.
            let (x0, y0, x1, y1) = (
                r.x0.unsigned_abs(),
                r.y0.unsigned_abs(),
                r.x1.unsigned_abs(),
                r.y1.unsigned_abs(),
            );
            let at = |x: u32, y: u32| {
                usize::try_from(y)
                    .ok()
                    .and_then(|y| y.checked_mul(stride))
                    .and_then(|row| row.checked_add(usize::try_from(x).ok()?))
            };
            // The first pixel read, and one past the last: the end of the bottom row.
            let (Some(first), Some(end)) = (at(x0, y0), at(x1, y1.saturating_sub(1))) else {
                continue;
            };
            if first >= end || end > pixels.len() {
                continue;
            }
            let area = D3D11_BOX {
                left: x0,
                top: y0,
                front: 0,
                right: x1,
                bottom: y1,
                back: 1,
            };
            // SAFETY: the call reads rows of `x1 - x0` pixels, `w` apart, from `first` through
            // `end`, which is inside `pixels` (checked above); it copies them before it returns.
            unsafe {
                self.gpu.context.UpdateSubresource(
                    &back,
                    0,
                    Some(&raw const area),
                    pixels[first..end].as_ptr().cast(),
                    w * 4,
                    0,
                );
            }
        }
        drop(back);
        let mut dirty = [RECT::default(); MAX_RECTS];
        let named = plan.dirty.as_ref().map_or(&[][..], Rects::as_slice);
        for (slot, r) in dirty.iter_mut().zip(named) {
            *slot = RECT {
                left: r.x0,
                top: r.y0,
                right: r.x1,
                bottom: r.y1,
            };
        }
        let params = DXGI_PRESENT_PARAMETERS {
            // None: all of the buffer, as the first present after new buffers must be.
            DirtyRectsCount: u32::try_from(named.len()).unwrap_or(0),
            pDirtyRects: if named.is_empty() {
                null_mut()
            } else {
                dirty.as_mut_ptr()
            },
            pScrollRect: null_mut(),
            pScrollOffset: null_mut(),
        };
        // SAFETY: the parameters and the rectangles they point to outlive the call. Sync interval
        // 1: one present per refresh at most.
        let hr = unsafe { self.swap.Present1(1, DXGI_PRESENT(0), &raw const params) };
        if hr.is_err() {
            return Err(Lost);
        }
        self.stale.presented(damage);
        Ok(())
    }
}

/// Whether the tests run under Wine (T2), whose `DirectComposition` is a stub, rather than Windows
/// (T3), which always has WARP and `DirectComposition`.
#[cfg(test)]
pub(super) fn on_wine() -> bool {
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    // SAFETY: looking up an export of a module every process has loaded.
    unsafe {
        let ntdll = GetModuleHandleW(windows_sys::core::w!("ntdll.dll"));
        !ntdll.is_null() && GetProcAddress(ntdll, c"wine_get_version".as_ptr().cast()).is_some()
    }
}

/// A swap chain has at least one pixel each way.
fn at_least_one(size: SizePx) -> SizePx {
    SizePx {
        w: size.w.max(1),
        h: size.h.max(1),
    }
}

#[cfg(test)]
mod tests {
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM;

    use super::*;

    fn rect(x0: i32, y0: i32, x1: i32, y1: i32) -> RectPx {
        RectPx { x0, y0, x1, y1 }
    }

    #[test]
    fn rects_skip_empties_and_repeats_and_merge_past_the_cap() {
        let mut r = Rects::default();
        r.push(rect(0, 0, 0, 5));
        r.push(rect(1, 1, 2, 2));
        r.push(rect(1, 1, 2, 2));
        assert_eq!(r.as_slice(), [rect(1, 1, 2, 2)]);
        for i in 0..20 {
            r.push(rect(i, 0, i + 1, 1));
        }
        // The 17th made one box over all 17; the four after it were added beside it.
        assert_eq!(
            r.as_slice(),
            [
                rect(0, 0, 16, 2),
                rect(16, 0, 17, 1),
                rect(17, 0, 18, 1),
                rect(18, 0, 19, 1),
                rect(19, 0, 20, 1),
            ]
        );
        let clipped = Rects::whole(SizePx { w: 10, h: 10 });
        assert_eq!(clipped.as_slice(), [rect(0, 0, 10, 10)]);
        let mut big = Rects::default();
        big.push(rect(-5, -5, 50, 3));
        assert_eq!(
            big.clipped(SizePx { w: 10, h: 10 }).as_slice(),
            [rect(0, 0, 10, 3)]
        );
        assert_eq!(big.clipped(SizePx { w: 10, h: 10 }).area(), 30);
    }

    /// Each buffer gets what it lacks; `Present1` hears only what changed on screen.
    #[test]
    fn both_buffers_are_brought_up_to_the_canvas() {
        let size = SizePx { w: 10, h: 10 };
        let whole = Rects::whole(size);
        let one = |r| {
            let mut d = Rects::default();
            d.push(r);
            d
        };
        let (a, b) = (one(rect(0, 0, 2, 2)), one(rect(5, 5, 6, 6)));
        let mut s = Staleness::new(size);
        // New buffers: all of the first one, named as a whole present, even without damage.
        let plan = s.plan(&Rects::default()).expect("new buffers are due");
        assert_eq!((plan.upload, plan.dirty), (whole, None));
        s.presented(&Rects::default());
        assert!(
            s.plan(&Rects::default()).is_none(),
            "nothing changed on screen"
        );
        // The second buffer is blank too: all of it, though only `a` changed on screen.
        let plan = s.plan(&a).expect("damage");
        assert_eq!((plan.upload, plan.dirty), (whole, Some(a)));
        s.presented(&a);
        // From here, the frame before last's damage and this one's.
        let plan = s.plan(&b).expect("damage");
        assert_eq!((plan.upload, plan.dirty), (a.union(&b), Some(b)));
        s.presented(&b);
        let plan = s.plan(&a).expect("damage");
        assert_eq!((plan.upload, plan.dirty), (b.union(&a), Some(a)));
        // Damage over everything presents whole.
        let mut most = Rects::default();
        most.push(rect(0, 0, 10, 6));
        most.push(rect(0, 4, 10, 10));
        assert_eq!(
            s.plan(&most).map(|p| (p.upload, p.dirty)),
            Some((whole, None))
        );
    }

    #[test]
    fn a_second_loss_within_a_minute_moves_to_warp() {
        let now = Instant::now();
        assert_eq!(after_loss(None, now), Device::Hardware);
        assert_eq!(
            after_loss(Some(now), now + Duration::from_secs(59)),
            Device::Warp
        );
        assert_eq!(after_loss(Some(now), now + LOSS_WINDOW), Device::Hardware);
    }

    #[test]
    fn the_basic_render_driver_is_software_whatever_its_flags() {
        let desc = |vendor, device, flags| DXGI_ADAPTER_DESC1 {
            VendorId: vendor,
            DeviceId: device,
            Flags: flags,
            ..DXGI_ADAPTER_DESC1::default()
        };
        assert!(is_software(&desc(MICROSOFT, BASIC_RENDER, 0)));
        assert!(is_software(&desc(0x10DE, 0x2504, 2)));
        assert!(!is_software(&desc(0x10DE, 0x2504, 0)));
        assert!(!is_software(&desc(MICROSOFT, 0x1234, 0)));
    }

    /// T3 (the G1 runner): records DWM's adapter and the swap-chain formats (TD-0009 §3's
    /// verifications), and checks WARP makes a presenter: what `--materials glass` runs on. Under
    /// Wine, only that every step fails cleanly.
    #[test]
    fn the_runner_s_adapter_and_warp() {
        println!("adapter 0: {:?}", adapter_summary());
        let gpu = Gpu::new(Device::Warp);
        if on_wine() {
            println!("Wine: WARP and DirectComposition devices {}", gpu.is_some());
            return;
        }
        let gpu = gpu.expect("Windows always has WARP and DirectComposition");
        let size = SizePx { w: 64, h: 32 };
        for (name, format) in [
            ("B8G8R8A8_UNORM", DXGI_FORMAT_B8G8R8A8_UNORM),
            ("R8G8B8A8_UNORM", DXGI_FORMAT_R8G8B8A8_UNORM),
        ] {
            let made = gpu.swap_chain(size, format);
            println!(
                "premultiplied composition swap chain, {name}: {}",
                made.as_ref()
                    .map_or_else(|e| format!("{e}"), |_| "ok".into())
            );
            if format == DXGI_FORMAT_B8G8R8A8_UNORM {
                assert!(made.is_ok(), "the presenter's own format");
            }
        }
        // Hardware is refused on a software adapter (#30j).
        if adapter0(exports().expect("the DLLs")).is_some_and(|(_, desc)| is_software(&desc)) {
            assert!(Gpu::new(Device::Hardware).is_none());
            assert!(Gpu::open(Compose::Hardware).is_none());
            assert!(Gpu::open(Compose::Any).is_some_and(|g| g.warp));
        }
    }
}
