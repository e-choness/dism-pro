//! Direct3D 11 + DXGI + DirectComposition (glass presentation) and Direct2D (the in-app acrylic
//! gate), all resolved at run time: none of `d3d11`, `dxgi`, `dcomp` or `d2d1` is in base WinPE
//! (S-01 facts, run 41589907500), and a static import would stop the exe from starting there.
//! Without them the window falls back to GDI and the solid materials.

use std::{ffi::c_void, time::Instant};

use windows::{
    Win32::{
        Foundation::{HMODULE, HWND},
        Graphics::{
            Direct2D::{
                CLSID_D2D1GaussianBlur, CLSID_D2D1Scale,
                Common::{
                    D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F,
                    D2D1_COMPOSITE_MODE_SOURCE_COPY, D2D1_PIXEL_FORMAT,
                },
                D2D1_BITMAP_OPTIONS_NONE, D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1,
                D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_FACTORY_OPTIONS, D2D1_FACTORY_TYPE,
                D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION,
                D2D1_INTERPOLATION_MODE_LINEAR, D2D1_PROPERTY_TYPE_FLOAT,
                D2D1_PROPERTY_TYPE_VECTOR2, D2D1_SCALE_PROP_SCALE, ID2D1DeviceContext,
                ID2D1Factory1,
            },
            Direct3D::{
                D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL,
            },
            Direct3D11::{
                D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_FLAG, D3D11_QUERY_DESC,
                D3D11_QUERY_EVENT, D3D11_SDK_VERSION, ID3D11Device, ID3D11DeviceContext,
                ID3D11Query, ID3D11Texture2D,
            },
            DirectComposition::{IDCompositionDevice, IDCompositionTarget, IDCompositionVisual},
            Dxgi::{
                Common::{
                    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_UNKNOWN,
                    DXGI_SAMPLE_DESC,
                },
                DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG,
                DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIDevice,
                IDXGIFactory2, IDXGISwapChain1,
            },
        },
        System::LibraryLoader::{GetProcAddress, LoadLibraryW},
    },
    core::{BOOL, GUID, HRESULT, Interface, PCSTR, PCWSTR, Result, s, w},
};

use crate::canvas::Canvas;

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
type CreateComposition =
    unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT;
type CreateD2D = unsafe extern "system" fn(
    D2D1_FACTORY_TYPE,
    *const GUID,
    *const D2D1_FACTORY_OPTIONS,
    *mut *mut c_void,
) -> HRESULT;

fn missing(what: &str) -> windows::core::Error {
    windows::core::Error::new(windows::Win32::Foundation::E_NOINTERFACE, what)
}

/// `GetProcAddress(LoadLibraryW(dll), name)`; `None` when either is missing (WinPE).
fn resolve(dll: PCWSTR, name: PCSTR) -> Option<unsafe extern "system" fn() -> isize> {
    // SAFETY: loading a system DLL by name and looking up an export.
    unsafe { GetProcAddress(LoadLibraryW(dll).ok()?, name) }
}

pub struct Gpu {
    pub device: ID3D11Device,
    pub ctx: ID3D11DeviceContext,
    dxgi: IDXGIDevice,
    factory: IDXGIFactory2,
    pub dcomp: IDCompositionDevice,
    pub warp: bool,
}

impl Gpu {
    /// A `D3D11` device (hardware, else WARP; `warp` forces WARP) and a DirectComposition device.
    pub fn new(warp: bool) -> Result<Self> {
        let f = resolve(w!("d3d11.dll"), s!("D3D11CreateDevice"))
            .ok_or_else(|| missing("d3d11.dll"))?;
        // SAFETY: the export has this signature (d3d11.h).
        let create: CreateDevice = unsafe { std::mem::transmute(f) };
        let drivers: &[D3D_DRIVER_TYPE] = if warp {
            &[D3D_DRIVER_TYPE_WARP]
        } else {
            &[D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP]
        };
        let mut made = None;
        for &driver in drivers {
            let (mut dev, mut ctx) = (std::ptr::null_mut(), std::ptr::null_mut());
            // SAFETY: valid arguments; out-pointers to locals; no adapter, default feature levels.
            let hr = unsafe {
                create(
                    std::ptr::null_mut(),
                    driver,
                    HMODULE::default(),
                    D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                    std::ptr::null(),
                    0,
                    D3D11_SDK_VERSION,
                    &raw mut dev,
                    std::ptr::null_mut(),
                    &raw mut ctx,
                )
            };
            if hr.is_ok() && !dev.is_null() && !ctx.is_null() {
                // SAFETY: both pointers are owned references the call just returned.
                made = Some(unsafe {
                    (
                        ID3D11Device::from_raw(dev),
                        ID3D11DeviceContext::from_raw(ctx),
                        driver == D3D_DRIVER_TYPE_WARP,
                    )
                });
                break;
            }
        }
        let (device, ctx, warp) = made.ok_or_else(|| missing("no D3D11 device"))?;
        let dxgi: IDXGIDevice = device.cast()?;
        // SAFETY: plain COM calls on live objects.
        let factory: IDXGIFactory2 = unsafe { dxgi.GetAdapter()?.GetParent()? };
        let f = resolve(w!("dcomp.dll"), s!("DCompositionCreateDevice"))
            .ok_or_else(|| missing("dcomp.dll"))?;
        // SAFETY: the export has this signature (dcomp.h).
        let create: CreateComposition = unsafe { std::mem::transmute(f) };
        let mut raw = std::ptr::null_mut();
        // SAFETY: a live DXGI device, the interface id we ask for, and an out-pointer to a local.
        unsafe { create(dxgi.as_raw(), &IDCompositionDevice::IID, &raw mut raw).ok()? };
        // SAFETY: an owned reference of the requested interface.
        let dcomp = unsafe { IDCompositionDevice::from_raw(raw) };
        Ok(Self {
            device,
            ctx,
            dxgi,
            factory,
            dcomp,
            warp,
        })
    }

    /// Waits until the GPU (or WARP's worker threads) has finished everything submitted so far.
    pub fn finish(&self) -> Result<()> {
        let desc = D3D11_QUERY_DESC {
            Query: D3D11_QUERY_EVENT,
            MiscFlags: 0,
        };
        let mut q: Option<ID3D11Query> = None;
        // SAFETY: valid descriptor, out-pointer to a local.
        unsafe { self.device.CreateQuery(&raw const desc, Some(&raw mut q))? };
        let q = q.ok_or_else(|| missing("query"))?;
        // SAFETY: a live query; GetData writes a BOOL once the event has passed (and flushes).
        unsafe {
            self.ctx.End(&q);
            let mut done = BOOL(0);
            while done.0 == 0 {
                self.ctx.GetData(&q, Some((&raw mut done).cast()), 4, 0)?;
            }
        }
        Ok(())
    }
}

/// A window's DirectComposition content: a premultiplied-alpha flip swap chain the software canvas
/// is uploaded into. DWM composes it over the Mica or Acrylic backdrop.
pub struct Surface {
    swap: IDXGISwapChain1,
    _target: IDCompositionTarget,
    _visual: IDCompositionVisual,
    w: u32,
    h: u32,
    staging: Vec<u8>,
}

impl Surface {
    pub fn new(gpu: &Gpu, hwnd: HWND, w: u32, h: u32) -> Result<Self> {
        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: w.max(1),
            Height: h.max(1),
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            Stereo: false.into(),
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
            AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
            Flags: 0,
        };
        // SAFETY: live devices, a valid descriptor and a live window of this thread.
        unsafe {
            let swap =
                gpu.factory
                    .CreateSwapChainForComposition(&gpu.device, &raw const desc, None)?;
            let target = gpu.dcomp.CreateTargetForHwnd(hwnd, true)?;
            let visual = gpu.dcomp.CreateVisual()?;
            visual.SetContent(&swap)?;
            target.SetRoot(&visual)?;
            gpu.dcomp.Commit()?;
            Ok(Self {
                swap,
                _target: target,
                _visual: visual,
                w: desc.Width,
                h: desc.Height,
                staging: Vec::new(),
            })
        }
    }

    pub fn resize(&mut self, w: u32, h: u32) -> Result<()> {
        let (w, h) = (w.max(1), h.max(1));
        if (w, h) != (self.w, self.h) {
            // SAFETY: no outstanding references to the buffers (each present releases its texture).
            unsafe {
                self.swap
                    .ResizeBuffers(0, w, h, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0))?;
            };
            (self.w, self.h) = (w, h);
        }
        Ok(())
    }

    /// Uploads the canvas (premultiplied) into the back buffer and presents it.
    pub fn present(&mut self, gpu: &Gpu, c: &Canvas) -> Result<()> {
        if (c.width(), c.height()) != (self.w, self.h) {
            self.resize(c.width(), c.height())?;
        }
        self.staging.resize((self.w * self.h * 4) as usize, 0);
        c.to_bgra(&mut self.staging);
        // SAFETY: the staging buffer holds w × h BGRA pixels, matching the back buffer.
        unsafe {
            let back: ID3D11Texture2D = self.swap.GetBuffer(0)?;
            gpu.ctx
                .UpdateSubresource(&back, 0, None, self.staging.as_ptr().cast(), self.w * 4, 0);
            self.swap.Present(1, DXGI_PRESENT(0)).ok()
        }
    }
}

/// The ADR-0002 acrylic gate: one recomputation of the in-app acrylic of a `w × h` drawer region
/// (¼ downsample → Gaussian blur σ 10 at ¼ → upsample → tint) with Direct2D on WARP, against a plain
/// Direct2D fill of the same region. Medians of `rounds` interleaved runs, in ms, plus the upload
/// of the software frame that option A would do first.
pub struct AcrylicTimes {
    pub blur: f64,
    pub fill: f64,
    pub upload: f64,
}

pub fn acrylic_bench(pixels: &[u8], w: u32, h: u32, rounds: usize) -> Result<AcrylicTimes> {
    let gpu = Gpu::new(true)?;
    let f = resolve(w!("d2d1.dll"), s!("D2D1CreateFactory")).ok_or_else(|| missing("d2d1.dll"))?;
    // SAFETY: the export has this signature (d2d1.h).
    let create: CreateD2D = unsafe { std::mem::transmute(f) };
    let mut raw = std::ptr::null_mut();
    // SAFETY: valid arguments; out-pointer to a local.
    unsafe {
        create(
            D2D1_FACTORY_TYPE_SINGLE_THREADED,
            &ID2D1Factory1::IID,
            std::ptr::null(),
            &raw mut raw,
        )
        .ok()?;
    };
    // SAFETY: an owned reference of the requested interface.
    let factory = unsafe { ID2D1Factory1::from_raw(raw) };
    // SAFETY: plain COM calls on live objects with valid descriptors and buffers of the sizes given.
    unsafe {
        let dc: ID2D1DeviceContext = factory
            .CreateDevice(&gpu.dxgi)?
            .CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
        let format = D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        };
        let props = |options| D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: format,
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: options,
            colorContext: std::mem::ManuallyDrop::new(None),
        };
        let size = D2D_SIZE_U {
            width: w,
            height: h,
        };
        let src_props = props(D2D1_BITMAP_OPTIONS_NONE);
        let source = dc.CreateBitmap(
            size,
            Some(pixels.as_ptr().cast()),
            w * 4,
            &raw const src_props,
        )?;
        let target_props = props(D2D1_BITMAP_OPTIONS_TARGET);
        let target = dc.CreateBitmap(size, None, 0, &raw const target_props)?;
        dc.SetTarget(&target);

        let vec2 = |v: f32| [v.to_le_bytes(), v.to_le_bytes()].concat();
        let down = dc.CreateEffect(&CLSID_D2D1Scale)?;
        down.SetInput(0, &source, true);
        down.SetValue(
            D2D1_SCALE_PROP_SCALE.0 as u32,
            D2D1_PROPERTY_TYPE_VECTOR2,
            &vec2(0.25),
        )?;
        let blur = dc.CreateEffect(&CLSID_D2D1GaussianBlur)?;
        blur.SetInput(0, &down.GetOutput()?, true);
        blur.SetValue(
            D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION.0 as u32,
            D2D1_PROPERTY_TYPE_FLOAT,
            &10.0_f32.to_le_bytes(),
        )?;
        let up = dc.CreateEffect(&CLSID_D2D1Scale)?;
        up.SetInput(0, &blur.GetOutput()?, true);
        up.SetValue(
            D2D1_SCALE_PROP_SCALE.0 as u32,
            D2D1_PROPERTY_TYPE_VECTOR2,
            &vec2(4.0),
        )?;
        let output = up.GetOutput()?;
        // The drawer's light tint (F9FAFD at 189/255), as in the software path.
        let tint_color = D2D1_COLOR_F {
            r: 0.976,
            g: 0.98,
            b: 0.992,
            a: 0.741,
        };
        let solid_color = D2D1_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 1.0,
        };
        let tint = dc.CreateSolidColorBrush(&raw const tint_color, None)?;
        let solid = dc.CreateSolidColorBrush(&raw const solid_color, None)?;
        let rect = D2D_RECT_F {
            left: 0.0,
            top: 0.0,
            right: w as f32,
            bottom: h as f32,
        };

        let time = |f: &dyn Fn() -> Result<()>| -> Result<f64> {
            let t = Instant::now();
            f()?;
            gpu.finish()?;
            Ok(t.elapsed().as_secs_f64() * 1e3)
        };
        let acrylic = || -> Result<()> {
            dc.BeginDraw();
            dc.DrawImage(
                &output,
                None,
                None,
                D2D1_INTERPOLATION_MODE_LINEAR,
                D2D1_COMPOSITE_MODE_SOURCE_COPY,
            );
            dc.FillRectangle(&raw const rect, &tint);
            dc.EndDraw(None, None)
        };
        let fill = || -> Result<()> {
            dc.BeginDraw();
            dc.FillRectangle(&raw const rect, &solid);
            dc.EndDraw(None, None)
        };
        let upload = || source.CopyFromMemory(None, pixels.as_ptr().cast(), w * 4);
        for _ in 0..3 {
            time(&acrylic)?;
            time(&fill)?;
        }
        let (mut a, mut f, mut u) = (Vec::new(), Vec::new(), Vec::new());
        for _ in 0..rounds {
            a.push(time(&acrylic)?);
            f.push(time(&fill)?);
            u.push(time(&upload)?);
        }
        let med = |v: &mut Vec<f64>| scene::stats::median(v).unwrap_or(f64::NAN);
        Ok(AcrylicTimes {
            blur: med(&mut a),
            fill: med(&mut f),
            upload: med(&mut u),
        })
    }
}
