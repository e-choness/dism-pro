//! The system's file dialog (TD-0009 A3.5 (12)): `IFileOpenDialog`, choosing a folder or a file,
//! owned by the main window, with the in-app picker behind it where Windows has none.
//!
//! - **COM, at run time.** `ole32.dll` is loaded when a dialog is wanted (`CoInitializeEx`,
//!   `CoCreateInstance`, `CoUninitialize`, `CoTaskMemFree`), and `shell32.dll` only for the start
//!   folder (`SHCreateItemFromParsingName`), so the executable imports neither `shell32.dll` nor
//!   `comdlg32.dll`, and a `WinPE` without them still starts (`xtask pe-compat` checks the probes).
//! - **The apartment.** Each [`pick`] runs in a [`ComApartment`] of its own on the UI thread, a
//!   single-threaded one entered and left around the call, because the dialog's owner window lives
//!   on that thread (A3.10 item 9). **This amends TD-0009 §5.3**, whose UI thread was an STA for
//!   the process's life: the thread is in no apartment between picks. Nothing else on it needs
//!   one: `UISettings` is never activated (`env.rs` reads the registry values behind it), and
//!   AccessKit's provider is a server-side one that UI Automation calls on its own threads. A
//!   thread already in the multithreaded apartment (`RPC_E_CHANGED_MODE`) can't host the dialog:
//!   [`PickError::Unavailable`].
//! - **The dialog.** `CLSID_FileOpenDialog` in process. Its own options stay (an open dialog's
//!   include `FOS_FILEMUSTEXIST`, which a file pick wants), with `FOS_FORCEFILESYSTEM`,
//!   `FOS_PATHMUSTEXIST`, `FOS_NOCHANGEDIR` and `FOS_DONTADDTORECENT` added, and `FOS_PICKFOLDERS`
//!   to choose a folder; a file's types through `SetFileTypes`, the start through
//!   `SetDefaultFolder` (so the folder the user last chose wins, as Windows intends). `Show`
//!   returning `ERROR_CANCELLED` is `Ok(None)`; otherwise the result's `SIGDN_FILESYSPATH`, freed
//!   with `CoTaskMemFree`.
//! - **Unavailable.** Anything that keeps the dialog from showing (the class not registered, an
//!   option it refuses, `Show` failing with anything but a cancel) is
//!   [`PickError::Unavailable`]: the window remembers it for the session, answering every later
//!   request with it at once, and the model opens the in-app picker instead (A3.10 item 9: no
//!   setting). [`PickError::Failed`] is a dialog that ran and gave no path, or a request it can't
//!   take (a NUL in the title or a type). A window can be told so from the start
//!   ([`WindowCx::without_file_dialog`](super::WindowCx::without_file_dialog)): the picker probe
//!   and the fallback's tests take that same path on a Windows that has the dialog.
//! - **Never inside an event.** `Show` runs a modal loop, and a handler must not pump messages.
//!   So the handler asks with [`WindowCx::pick`](super::WindowCx::pick), the window runs the
//!   dialog once the handler has returned and nothing of it is borrowed, and the answer comes
//!   back through [`Handler::picked`](super::Handler::picked). The modal loop still dispatches
//!   the window's paints, timers and wakes meanwhile. A5's `Effect::Pick` replaces the request.
//! - **Shell extensions** load into the process with the dialog (A3.8): `FOS_FORCEFILESYSTEM`
//!   keeps it to file-system items, a crash reaches the crash handler, and the in-app picker is
//!   always there.
//!
//! [`drives`] lists the drive roots, for the in-app picker's drive list.

use std::{
    cell::{Cell, RefCell},
    ffi::{OsString, c_void},
    marker::PhantomData,
    os::windows::ffi::OsStringExt as _,
    path::{Path, PathBuf},
    ptr::{self, NonNull},
};

use dpro_core::OsError;
use windows_sys::{
    Win32::{
        Foundation::{
            E_UNEXPECTED, ERROR_CANCELLED, ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED, HWND,
        },
        Storage::FileSystem::GetLogicalDriveStringsW,
        System::Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE},
        UI::Shell::{
            Common::COMDLG_FILTERSPEC, FILEOPENDIALOGOPTIONS, FOS_DONTADDTORECENT,
            FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS,
            FileOpenDialog, SIGDN, SIGDN_FILESYSPATH,
        },
    },
    core::{GUID, HRESULT, PCWSTR, PWSTR},
};

use crate::{
    dll::{self, Proc},
    error::Error,
    wide::{self, WideCString},
};

/// The window a dialog is modal to; [`Hwnd::NONE`] for none (the tests).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hwnd(pub(crate) HWND);

impl Hwnd {
    /// No owner.
    pub const NONE: Self = Self(std::ptr::null_mut());
}

/// What to pick, and how the dialog is titled.
#[derive(Clone, Copy, Debug)]
pub struct Pick<'a> {
    pub mode: PickMode<'a>,
    /// `ui-picker-folder-title` or `ui-picker-file-title`, resolved.
    pub title: &'a str,
    /// The folder it opens in; `None` for the dialog's own choice.
    pub start: Option<&'a Path>,
}

/// A folder, or a file of some types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickMode<'a> {
    Folder,
    /// Each type a label and its patterns (`("Windows images", "*.wim;*.esd")`), the first one
    /// chosen at first.
    File {
        types: &'a [(&'a str, &'a str)],
    },
}

/// Why no path came back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PickError {
    /// There's no system dialog here (`WinPE`, the class not registered, a thread in the
    /// multithreaded apartment): the in-app picker opens instead.
    #[error("the system file dialog is unavailable: {0}")]
    Unavailable(OsError),
    /// The dialog ran and failed (its result had no file-system path), or couldn't take the
    /// request (a NUL in its title or a type).
    #[error("the system file dialog failed: {0}")]
    Failed(OsError),
}

/// A pick's answer: the path, or `None` if the user cancelled.
pub type Picked = Result<Option<PathBuf>, PickError>;

/// Shows the system dialog, modal to `owner`, and waits for the user. `Ok(None)` if they
/// cancelled. Never call it while handling an event: ask with
/// [`WindowCx::pick`](super::WindowCx::pick).
///
/// Enters a single-threaded COM apartment on this thread for the call, and leaves it before
/// returning; the dialog's modal loop dispatches this thread's messages meanwhile. Changes
/// nothing on disk: the dialog is opened with `FOS_NOCHANGEDIR` and `FOS_DONTADDTORECENT`.
///
/// # Errors
/// [`PickError::Unavailable`] where there's no dialog; [`PickError::Failed`] if it failed.
pub fn pick(owner: Hwnd, pick: &Pick<'_>) -> Picked {
    // Made before the dialog and released after it, which may read them until then.
    let strings = Strings::new(pick)?;
    let ole32 = Ole32::load().map_err(PickError::Unavailable)?;
    // Declared before every object, so it's left last: they're released inside it.
    let _apartment = ComApartment::enter(&ole32)?;
    let dialog = take::<DialogVtbl>("CoCreateInstance", |out| {
        // SAFETY: the class and interface IDs are live constants, `out` a live out parameter;
        // no aggregation.
        unsafe {
            (ole32.create)(
                &FileOpenDialog,
                ptr::null_mut(),
                CLSCTX_INPROC_SERVER,
                &DialogVtbl::IID,
                out,
            )
        }
    })
    .map_err(PickError::Unavailable)?;
    set_up(&dialog, pick, &strings)?;
    // SAFETY: the dialog is live; `owner` is a window of this thread, or null. Runs the modal
    // loop until the user answers.
    let hr = unsafe { (dialog.vtbl().show)(dialog.as_raw(), owner.0) };
    if !shown(hr)? {
        return Ok(None);
    }
    let item = take::<ItemVtbl>("IFileDialog::GetResult", |out| {
        // SAFETY: the dialog is live and was shown; `out` a live out parameter.
        unsafe { (dialog.vtbl().get_result)(dialog.as_raw(), out) }
    })
    .map_err(PickError::Failed)?;
    path_of(&ole32, &item).map(Some).map_err(PickError::Failed)
}

/// The drive roots (`C:\`, `D:\`), in letter order, for the in-app picker's drive list; empty if
/// Windows wouldn't say (`GetLogicalDriveStringsW`).
#[must_use]
pub fn drives() -> Vec<PathBuf> {
    // 26 letters of `X:\` and their NULs, and the list's own NUL.
    let mut list = [0_u16; 26 * 4 + 1];
    let len = u32::try_from(list.len()).unwrap_or(0);
    // SAFETY: the buffer is live and `len` units long; the call writes within it.
    let filled = unsafe { GetLogicalDriveStringsW(len, list.as_mut_ptr()) };
    // 0 is a failure, and more than `len` (the length it would need) can't be with 26 letters.
    usize::try_from(filled)
        .ok()
        .and_then(|n| list.get(..n))
        .map_or_else(Vec::new, roots)
}

/// The roots in `GetLogicalDriveStringsW`'s list, each NUL-terminated, in letter order.
fn roots(list: &[u16]) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = list
        .split(|&u| u == 0)
        .filter(|root| !root.is_empty())
        .map(|root| PathBuf::from(OsString::from_wide(root)))
        .collect();
    roots.sort();
    roots
}

/// A single-threaded COM apartment on this thread, entered for one dialog: `CoInitializeEx`
/// with `COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE`, balanced by `CoUninitialize` when
/// it's dropped, on the same thread (it's neither `Send` nor `Sync`). Entering it again while it
/// holds (`S_FALSE`) is fine: each balances its own.
#[derive(Debug)]
pub struct ComApartment {
    uninitialize: CoUninitialize,
    _thread: PhantomData<*const ()>,
}

impl ComApartment {
    /// Enters the apartment.
    ///
    /// # Errors
    /// [`PickError::Unavailable`]: `ole32.dll` isn't there, or this thread is in the
    /// multithreaded apartment (`RPC_E_CHANGED_MODE`).
    pub fn sta() -> Result<Self, PickError> {
        Self::enter(&Ole32::load().map_err(PickError::Unavailable)?)
    }

    fn enter(ole32: &Ole32) -> Result<Self, PickError> {
        let mode = (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE).cast_unsigned();
        // SAFETY: no preconditions. A success (S_OK, or S_FALSE when this thread is in the
        // apartment already) is balanced by `drop`, on this thread: the guard isn't Send.
        let hr = unsafe { (ole32.initialize)(ptr::null(), mode) };
        if hr < 0 {
            return Err(PickError::Unavailable(hresult(hr, "CoInitializeEx")));
        }
        Ok(Self {
            uninitialize: ole32.uninitialize,
            _thread: PhantomData,
        })
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        // SAFETY: balances the successful CoInitializeEx of `enter`, on the same thread.
        unsafe { (self.uninitialize)() };
    }
}

// ole32 and shell32 are looked up when a dialog is wanted: every other run, and WinPE without
// them, never load them.
type CoInitializeEx = unsafe extern "system" fn(*const c_void, u32) -> HRESULT;
type CoUninitialize = unsafe extern "system" fn();
type CoCreateInstance = unsafe extern "system" fn(
    *const GUID,
    *mut c_void,
    u32,
    *const GUID,
    *mut *mut c_void,
) -> HRESULT;
type CoTaskMemFree = unsafe extern "system" fn(*const c_void);
type ShCreateItemFromParsingName =
    unsafe extern "system" fn(PCWSTR, *mut c_void, *const GUID, *mut *mut c_void) -> HRESULT;

/// ole32's calls a dialog makes.
#[derive(Clone, Copy, Debug)]
struct Ole32 {
    initialize: CoInitializeEx,
    uninitialize: CoUninitialize,
    create: CoCreateInstance,
    free: CoTaskMemFree,
}

impl Ole32 {
    /// Looks them up, from System32 only (`dll::system`).
    fn load() -> Result<Self, OsError> {
        let ole32 = dll::system("ole32.dll").map_err(|e| os_error(&e))?;
        let export = |name| dll::export(ole32, name).map_err(|e| os_error(&e));
        let (initialize, uninitialize, create, free) = (
            export(c"CoInitializeEx")?,
            export(c"CoUninitialize")?,
            export(c"CoCreateInstance")?,
            export(c"CoTaskMemFree")?,
        );
        // SAFETY: ole32's exports, with these signatures (combaseapi.h, objbase.h).
        unsafe {
            Ok(Self {
                initialize: std::mem::transmute::<Proc, CoInitializeEx>(initialize),
                uninitialize: std::mem::transmute::<Proc, CoUninitialize>(uninitialize),
                create: std::mem::transmute::<Proc, CoCreateInstance>(create),
                free: std::mem::transmute::<Proc, CoTaskMemFree>(free),
            })
        }
    }
}

/// A run-time lookup's failure, as the OS error it is.
fn os_error(e: &Error) -> OsError {
    match e {
        Error::Os(e) => *e,
        // The names are NUL-free literals, so only the OS fails a lookup.
        _ => OsError::from_win32(ERROR_INVALID_PARAMETER, "LoadLibraryExW"),
    }
}

/// An `HRESULT` as the OS error it is.
const fn hresult(code: HRESULT, api: &'static str) -> OsError {
    OsError { code, api }
}

/// `HRESULT_FROM_WIN32(ERROR_CANCELLED)`: the user closed the dialog without a pick.
const CANCELLED: u32 = 0x8007_0000 | ERROR_CANCELLED;

/// What `Show` returned: true if the user picked, false if they cancelled.
///
/// # Errors
/// [`PickError::Unavailable`] for any other failure (A3.5 (12)): the dialog can't run here.
fn shown(hr: HRESULT) -> Result<bool, PickError> {
    if hr >= 0 {
        Ok(true)
    } else if hr.cast_unsigned() == CANCELLED {
        Ok(false)
    } else {
        Err(PickError::Unavailable(hresult(hr, "IFileDialog::Show")))
    }
}

/// The strings a dialog is given: its title and a file's types, each NUL-terminated.
struct Strings {
    title: WideCString,
    /// What `specs` points into.
    _types: Vec<(WideCString, WideCString)>,
    specs: Vec<COMDLG_FILTERSPEC>,
}

impl Strings {
    /// The request's strings, checked before any COM.
    ///
    /// # Errors
    /// [`PickError::Failed`] for a NUL in the title or a type, which no dialog could show.
    fn new(pick: &Pick<'_>) -> Result<Self, PickError> {
        let title = WideCString::new(pick.title).map_err(|_| invalid("IFileDialog::SetTitle"))?;
        let types = match pick.mode {
            PickMode::Folder => Vec::new(),
            PickMode::File { types } => types
                .iter()
                .map(|&(label, patterns)| {
                    Ok((WideCString::new(label)?, WideCString::new(patterns)?))
                })
                .collect::<Result<Vec<_>, Error>>()
                .map_err(|_| invalid("IFileDialog::SetFileTypes"))?,
        };
        // The pointers are into the strings' heap buffers, which moving them doesn't move.
        let specs = types
            .iter()
            .map(|(label, patterns)| COMDLG_FILTERSPEC {
                pszName: label.as_ptr(),
                pszSpec: patterns.as_ptr(),
            })
            .collect();
        Ok(Self {
            title,
            _types: types,
            specs,
        })
    }
}

/// A request no dialog could take, as the call it would have failed.
fn invalid(api: &'static str) -> PickError {
    PickError::Failed(OsError::from_win32(ERROR_INVALID_PARAMETER, api))
}

/// Sets the dialog's options, title, types and start folder.
///
/// # Errors
/// [`PickError::Unavailable`] if it refuses one.
fn set_up(dialog: &Com<DialogVtbl>, pick: &Pick<'_>, strings: &Strings) -> Result<(), PickError> {
    let (vtbl, this) = (dialog.vtbl(), dialog.as_raw());
    let refused = |hr: HRESULT, api| {
        if hr < 0 {
            Err(PickError::Unavailable(hresult(hr, api)))
        } else {
            Ok(())
        }
    };

    let mut options: FILEOPENDIALOGOPTIONS = 0;
    // SAFETY: the dialog is live; `options` a live out parameter.
    let hr = unsafe { (vtbl.get_options)(this, &raw mut options) };
    refused(hr, "IFileDialog::GetOptions")?;
    options |= FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_NOCHANGEDIR | FOS_DONTADDTORECENT;
    if pick.mode == PickMode::Folder {
        options |= FOS_PICKFOLDERS;
    }
    // SAFETY: the dialog is live.
    let hr = unsafe { (vtbl.set_options)(this, options) };
    refused(hr, "IFileDialog::SetOptions")?;

    // SAFETY: the dialog is live; the title is NUL-terminated and outlives the dialog (`pick`).
    let hr = unsafe { (vtbl.set_title)(this, strings.title.as_ptr()) };
    refused(hr, "IFileDialog::SetTitle")?;

    if !strings.specs.is_empty() {
        let count =
            u32::try_from(strings.specs.len()).map_err(|_| invalid("IFileDialog::SetFileTypes"))?;
        // SAFETY: the dialog is live; `count` specs, whose strings outlive the dialog (`pick`).
        let hr = unsafe { (vtbl.set_file_types)(this, count, strings.specs.as_ptr()) };
        refused(hr, "IFileDialog::SetFileTypes")?;
    }

    if let Some(start) = pick.start {
        set_start(dialog, start);
    }
    Ok(())
}

/// Opens the dialog in `start` when there's no folder the user chose last, if the shell can
/// parse it; otherwise the dialog picks, as for no start at all.
fn set_start(dialog: &Com<DialogVtbl>, start: &Path) {
    let Ok(path) = WideCString::new(start) else {
        return;
    };
    let Some(create) = dll::system("shell32.dll")
        .and_then(|shell32| dll::export(shell32, c"SHCreateItemFromParsingName"))
        .ok()
        // SAFETY: shell32's export, with this signature (shobjidl_core.h).
        .map(|export| unsafe { std::mem::transmute::<Proc, ShCreateItemFromParsingName>(export) })
    else {
        return;
    };
    let Ok(folder) = take::<ItemVtbl>("SHCreateItemFromParsingName", |out| {
        // SAFETY: the path is NUL-terminated and read during the call; no bind context; the
        // interface ID is a live constant, `out` a live out parameter.
        unsafe { create(path.as_ptr(), ptr::null_mut(), &ItemVtbl::IID, out) }
    }) else {
        return;
    };
    // SAFETY: both are live; the dialog takes its own reference to the folder.
    let hr = unsafe { (dialog.vtbl().set_default_folder)(dialog.as_raw(), folder.as_raw()) };
    // ignore: a folder it refuses leaves the dialog's own choice, as a missing one does.
    let _ = hr;
}

/// The picked item's file-system path.
fn path_of(ole32: &Ole32, item: &Com<ItemVtbl>) -> Result<PathBuf, OsError> {
    const API: &str = "IShellItem::GetDisplayName";
    let mut name: PWSTR = ptr::null_mut();
    // SAFETY: the item is live; `name` a live out parameter, which a success fills with a string
    // the caller owns.
    let hr =
        unsafe { (item.vtbl().get_display_name)(item.as_raw(), SIGDN_FILESYSPATH, &raw mut name) };
    if hr < 0 {
        return Err(hresult(hr, API));
    }
    // SAFETY: after a success, `name` is null or a NUL-terminated path, which is never longer
    // than `MAX_OS_STRING`.
    let path = unsafe { wide::from_wide_ptr(name) };
    // SAFETY: the string the call allocated with the COM allocator, freed once; null is allowed.
    unsafe { (ole32.free)(name.cast_const().cast()) };
    match path {
        Ok(path) if !path.is_empty() => Ok(PathBuf::from(path)),
        _ => Err(hresult(E_UNEXPECTED, API)),
    }
}

/// A COM vtable, as this module calls it.
///
/// # Safety
/// The type is `#[repr(C)]`, its layout is the interface's that [`Self::IID`] names (methods past
/// the last one used may be left out), and it starts with `IUnknown`'s three methods.
unsafe trait Interface {
    const IID: GUID;
    fn unknown(&self) -> &IUnknownVtbl;
}

/// `IUnknown`'s methods, which every vtable starts with.
#[repr(C)]
struct IUnknownVtbl {
    _query_interface: *const c_void,
    _add_ref: *const c_void,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}

/// `IFileOpenDialog`'s vtable (`shobjidl_core.h`: `IModalWindow`, then `IFileDialog`) through
/// `GetResult`; the methods after it aren't called. Unused slots are opaque.
#[repr(C)]
struct DialogVtbl {
    unknown: IUnknownVtbl,
    show: unsafe extern "system" fn(*mut c_void, HWND) -> HRESULT,
    set_file_types:
        unsafe extern "system" fn(*mut c_void, u32, *const COMDLG_FILTERSPEC) -> HRESULT,
    _set_file_type_index: *const c_void,
    _get_file_type_index: *const c_void,
    _advise: *const c_void,
    _unadvise: *const c_void,
    set_options: unsafe extern "system" fn(*mut c_void, FILEOPENDIALOGOPTIONS) -> HRESULT,
    get_options: unsafe extern "system" fn(*mut c_void, *mut FILEOPENDIALOGOPTIONS) -> HRESULT,
    set_default_folder: unsafe extern "system" fn(*mut c_void, *mut c_void) -> HRESULT,
    _set_folder: *const c_void,
    _get_folder: *const c_void,
    _get_current_selection: *const c_void,
    _set_file_name: *const c_void,
    _get_file_name: *const c_void,
    set_title: unsafe extern "system" fn(*mut c_void, PCWSTR) -> HRESULT,
    _set_ok_button_label: *const c_void,
    _set_file_name_label: *const c_void,
    get_result: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
}

// SAFETY: `#[repr(C)]`, IFileOpenDialog's layout through GetResult, starting with IUnknown's.
unsafe impl Interface for DialogVtbl {
    /// `IID_IFileOpenDialog`.
    const IID: GUID = GUID::from_u128(0xd57c_7288_d4ad_4768_be02_9d96_9532_d960);

    fn unknown(&self) -> &IUnknownVtbl {
        &self.unknown
    }
}

/// `IShellItem`'s vtable through `GetDisplayName`.
#[repr(C)]
struct ItemVtbl {
    unknown: IUnknownVtbl,
    _bind_to_handler: *const c_void,
    _get_parent: *const c_void,
    get_display_name: unsafe extern "system" fn(*mut c_void, SIGDN, *mut PWSTR) -> HRESULT,
}

// SAFETY: `#[repr(C)]`, IShellItem's layout through GetDisplayName, starting with IUnknown's.
unsafe impl Interface for ItemVtbl {
    /// `IID_IShellItem`.
    const IID: GUID = GUID::from_u128(0x4382_6d1e_e718_42ee_bc55_a1e2_61c3_7bfe);

    fn unknown(&self) -> &IUnknownVtbl {
        &self.unknown
    }
}

/// An owned reference to a COM object with the interface `V`, released when dropped. Neither
/// `Send` nor `Sync`: it belongs to the apartment that made it.
struct Com<V: Interface> {
    this: NonNull<*const V>,
}

impl<V: Interface> Com<V> {
    /// Takes ownership of an interface pointer an out parameter returned; `None` for null.
    ///
    /// # Safety
    /// `raw` is null, or a reference the caller owns to an object with `V`'s interface.
    unsafe fn from_raw(raw: *mut c_void) -> Option<Self> {
        NonNull::new(raw.cast::<*const V>()).map(|this| Self { this })
    }

    fn as_raw(&self) -> *mut c_void {
        self.this.as_ptr().cast()
    }

    fn vtbl(&self) -> &V {
        // SAFETY: a COM object starts with its vtable's pointer, `V`'s by `from_raw`'s contract,
        // and the vtable outlives every reference to the object.
        unsafe { &**self.this.as_ptr() }
    }
}

impl<V: Interface> Drop for Com<V> {
    fn drop(&mut self) {
        let release = self.vtbl().unknown().release;
        // SAFETY: the one reference `from_raw` took, released once.
        unsafe { release(self.as_raw()) };
    }
}

/// Calls `call` with an out parameter for an interface pointer, and owns what it returns.
///
/// # Errors
/// The call's failure as `api`'s; `E_UNEXPECTED` for a success that returned nothing.
fn take<V: Interface>(
    api: &'static str,
    call: impl FnOnce(*mut *mut c_void) -> HRESULT,
) -> Result<Com<V>, OsError> {
    let mut raw = ptr::null_mut();
    let hr = call(&raw mut raw);
    if hr < 0 {
        return Err(hresult(hr, api));
    }
    // SAFETY: the call asked for `V`'s interface and succeeded, so `raw` is a reference it
    // handed to the caller, or null.
    unsafe { Com::from_raw(raw) }.ok_or(hresult(E_UNEXPECTED, api))
}

/// A pick the handler asked for, kept until it has returned.
#[derive(Debug)]
pub(crate) struct Request {
    folder: bool,
    types: Vec<(Box<str>, Box<str>)>,
    title: Box<str>,
    start: Option<PathBuf>,
}

impl Request {
    fn new(pick: &Pick<'_>) -> Self {
        let (folder, types) = match pick.mode {
            PickMode::Folder => (true, Vec::new()),
            PickMode::File { types } => (
                false,
                types.iter().map(|&(l, p)| (l.into(), p.into())).collect(),
            ),
        };
        Self {
            folder,
            types,
            title: pick.title.into(),
            start: pick.start.map(Path::to_path_buf),
        }
    }
}

/// A main window's picks: the one asked for, and whether the dialog is known to be unavailable.
#[derive(Debug, Default)]
pub(crate) struct Pending {
    request: RefCell<Option<Request>>,
    /// The dialog was [`PickError::Unavailable`] once: every later request is answered with this
    /// at once.
    unavailable: Cell<Option<OsError>>,
    /// A dialog is up: a request meanwhile is refused.
    picking: Cell<bool>,
}

impl Pending {
    /// Keeps `pick` for after the event. Returns false, keeping nothing, while another is kept or
    /// a dialog is up.
    pub(crate) fn request(&self, pick: &Pick<'_>) -> bool {
        if self.picking.get() {
            return false;
        }
        let Ok(mut slot) = self.request.try_borrow_mut() else {
            return false;
        };
        if slot.is_some() {
            return false;
        }
        *slot = Some(Request::new(pick));
        true
    }

    /// From now on every request is answered [`PickError::Unavailable`] at once, without COM, as
    /// where Windows has no dialog ([`WindowCx::without_file_dialog`](super::WindowCx::without_file_dialog)).
    /// A dialog already found unavailable keeps its own code.
    pub(crate) fn refuse(&self) {
        if self.unavailable.get().is_none() {
            self.unavailable.set(Some(OsError::from_win32(
                ERROR_NOT_SUPPORTED,
                "WindowCx::without_file_dialog",
            )));
        }
    }

    /// Once the handler has returned: runs the kept pick, modal to `owner`, and returns its
    /// answer; `None` if none was kept. Where the dialog was unavailable before, the answer is
    /// that, at once, without COM. Nothing is borrowed while the dialog's modal loop dispatches
    /// the window's messages, which may call this again (and find nothing kept).
    pub(crate) fn run(&self, owner: HWND) -> Option<Picked> {
        let request = self.request.try_borrow_mut().ok()?.take()?;
        if let Some(e) = self.unavailable.get() {
            return Some(Err(PickError::Unavailable(e)));
        }
        let types: Vec<(&str, &str)> = request.types.iter().map(|(l, p)| (&**l, &**p)).collect();
        let mode = if request.folder {
            PickMode::Folder
        } else {
            PickMode::File { types: &types }
        };
        self.picking.set(true);
        let picked = pick(
            Hwnd(owner),
            &Pick {
                mode,
                title: &request.title,
                start: request.start.as_deref(),
            },
        );
        self.picking.set(false);
        if let Err(PickError::Unavailable(e)) = picked {
            self.unavailable.set(Some(e));
        }
        Some(picked)
    }
}

/// T2 runs these under Wine; T3 and G1 open the dialog on Windows (`examples/file_dialog_t3.rs`).
/// Each COM test runs on a thread of its own, whose apartment nothing else touched. A test that
/// may show a dialog holds the window tests' lock, so its dialog takes no other test's
/// activation. The window's side (a pick after the event, its answer through `Handler::picked`)
/// is tested in `window.rs`.
#[cfg(test)]
pub(crate) mod tests {
    use std::{
        ffi::c_void,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        thread,
        time::{Duration, Instant},
    };

    use windows_sys::{
        Win32::UI::{
            Input::KeyboardAndMouse::VK_ESCAPE,
            WindowsAndMessaging::{FindWindowW, PostMessageW, WM_CLOSE, WM_KEYDOWN, WM_KEYUP},
        },
        core::HRESULT,
    };

    use super::*;
    use crate::{
        dll::{self, Proc},
        gui::window::serial::Serial,
        wide::WideCString,
    };

    const COINIT_MULTITHREADED: u32 = 0;
    const RPC_E_CHANGED_MODE: u32 = 0x8001_0106;
    const S_OK: HRESULT = 0;

    type CoInitializeEx = unsafe extern "system" fn(*const c_void, u32) -> HRESULT;
    type CoUninitialize = unsafe extern "system" fn();

    /// ole32's apartment calls, for the tests to enter the multithreaded one.
    fn ole32() -> (CoInitializeEx, CoUninitialize) {
        let ole32 = dll::system("ole32.dll").unwrap_or_else(|e| unreachable!("ole32: {e}"));
        let export = |name| dll::export(ole32, name).unwrap_or_else(|e| unreachable!("{e}"));
        // SAFETY: ole32's exports, with these documented signatures.
        unsafe {
            (
                std::mem::transmute::<Proc, CoInitializeEx>(export(c"CoInitializeEx")),
                std::mem::transmute::<Proc, CoUninitialize>(export(c"CoUninitialize")),
            )
        }
    }

    /// Runs `f` on a new thread and returns what it did.
    fn on_thread<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> R {
        thread::spawn(f)
            .join()
            .unwrap_or_else(|_| unreachable!("the test's thread panicked"))
    }

    #[test]
    fn the_apartment_balances_its_enters() {
        // Every test that enters COM takes the window tests' lock: under Wine 10 this test hung
        // with another's dialog up in 3 of 8 runs (WINE_CPU_TOPOLOGY=4), both waiting forever.
        let _serial = Serial::take();
        let mta = on_thread(|| {
            let outer = ComApartment::sta().unwrap_or_else(|e| unreachable!("{e}"));
            let inner = ComApartment::sta().unwrap_or_else(|e| unreachable!("{e}"));
            drop(inner);
            drop(outer);
            // Left wholly: the thread may join the multithreaded apartment as if new (S_OK, not
            // S_FALSE nor RPC_E_CHANGED_MODE).
            let (init, uninit) = ole32();
            // SAFETY: no preconditions; balanced below on this thread.
            let hr = unsafe { init(std::ptr::null(), COINIT_MULTITHREADED) };
            if hr >= 0 {
                // SAFETY: balances the success above, on this thread.
                unsafe { uninit() };
            }
            hr
        });
        assert_eq!(mta, S_OK);
    }

    #[test]
    fn a_thread_in_the_multithreaded_apartment_has_no_dialog() {
        let _serial = Serial::take();
        let entered = on_thread(|| {
            let (init, uninit) = ole32();
            // SAFETY: no preconditions; balanced below on this thread.
            let hr = unsafe { init(std::ptr::null(), COINIT_MULTITHREADED) };
            assert!(hr >= 0, "CoInitializeEx(MTA) failed: {hr:#x}");
            let entered = ComApartment::sta().map(drop);
            // SAFETY: balances the success above, on this thread.
            unsafe { uninit() };
            entered
        });
        match entered {
            Err(PickError::Unavailable(e)) => assert_eq!(e.hresult_bits(), RPC_E_CHANGED_MODE),
            other => panic!("expected Unavailable(RPC_E_CHANGED_MODE), got {other:?}"),
        }
    }

    /// Wine ships `IFileOpenDialog` (comdlg32's item dialog); where it can't be made, the answer
    /// must be Unavailable, which this records, never Failed.
    #[test]
    fn the_dialog_is_made_or_recorded_unavailable() {
        let _serial = Serial::take();
        let made = on_thread(|| {
            // A title no window has: Esc reaches a dialog only if one shows.
            let title = "dpro-win file_dialog test: made";
            let closer = close_with_esc(title);
            let picked = pick(
                Hwnd::NONE,
                &Pick {
                    mode: PickMode::Folder,
                    title,
                    start: None,
                },
            );
            closer.finish();
            picked
        });
        match made {
            Ok(None) => {}
            Err(PickError::Unavailable(e)) => {
                eprintln!("file_dialog: the system dialog is unavailable here: {e}");
            }
            other => panic!("expected Ok(None) or Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn esc_in_the_dialog_is_a_cancel() {
        let _serial = Serial::take();
        let picked = on_thread(|| {
            let title = "dpro-win file_dialog test: esc";
            let closer = close_with_esc(title);
            let types = [("Windows images", "*.wim;*.esd"), ("All files", "*.*")];
            let start = std::env::temp_dir();
            let picked = pick(
                Hwnd::NONE,
                &Pick {
                    mode: PickMode::File { types: &types },
                    title,
                    start: Some(&start),
                },
            );
            let found = closer.finish();
            (picked, found)
        });
        match picked {
            (Err(PickError::Unavailable(e)), _) => {
                eprintln!("file_dialog: the system dialog is unavailable here: {e}");
            }
            (picked, found) => {
                assert!(found, "the helper never found the dialog");
                assert_eq!(picked, Ok(None));
            }
        }
    }

    /// A request no dialog could show fails before any COM, so it's never taken for the dialog
    /// being unavailable: a NUL in the title, or in a file's type.
    #[test]
    fn a_nul_in_the_title_fails_before_the_dialog_shows() {
        const TYPES: &[(&str, &str)] = &[("Images", "*.wim"), ("dpro-win \0type", "*.*")];
        let picked = on_thread(|| {
            [
                (PickMode::Folder, "dpro-win file_dialog test: \0nul"),
                (PickMode::File { types: TYPES }, "dpro-win file_dialog test"),
            ]
            .map(|(mode, title)| {
                let pick_ = Pick {
                    mode,
                    title,
                    start: None,
                };
                pick(Hwnd::NONE, &pick_)
            })
        });
        assert_eq!(
            picked,
            [
                Err(invalid("IFileDialog::SetTitle")),
                Err(invalid("IFileDialog::SetFileTypes"))
            ]
        );
    }

    #[test]
    fn a_window_remembers_the_dialog_is_unavailable() {
        let _serial = Serial::take();
        let (first, second) = on_thread(|| {
            let pending = Pending::default();
            let pick = Pick {
                mode: PickMode::Folder,
                title: "dpro-win file_dialog test: remembered",
                start: None,
            };
            let (init, uninit) = ole32();
            // SAFETY: no preconditions; balanced below on this thread.
            let hr = unsafe { init(std::ptr::null(), COINIT_MULTITHREADED) };
            assert!(hr >= 0, "CoInitializeEx(MTA) failed: {hr:#x}");
            assert!(pending.request(&pick));
            let first = pending.run(std::ptr::null_mut());
            // SAFETY: balances the success above, on this thread.
            unsafe { uninit() };
            // The thread could enter a single-threaded apartment now, but the window remembers.
            assert!(pending.request(&pick));
            let second = pending.run(std::ptr::null_mut());
            (first, second)
        });
        let Some(Err(PickError::Unavailable(e))) = first else {
            panic!("expected Unavailable, got {first:?}");
        };
        assert_eq!(e.hresult_bits(), RPC_E_CHANGED_MODE);
        assert_eq!(second, Some(Err(PickError::Unavailable(e))));
    }

    #[test]
    fn a_second_request_waits_for_the_first() {
        let pending = Pending::default();
        // Known unavailable, so running the request answers at once and shows no dialog.
        let unavailable = OsError::from_win32(ERROR_CANCELLED, "test");
        pending.unavailable.set(Some(unavailable));
        let pick = Pick {
            mode: PickMode::Folder,
            title: "t",
            start: None,
        };
        assert!(pending.request(&pick));
        assert!(!pending.request(&pick), "one pick at a time");
        assert_eq!(
            pending.run(std::ptr::null_mut()),
            Some(Err(PickError::Unavailable(unavailable)))
        );
        assert!(pending.run(std::ptr::null_mut()).is_none(), "taken once");
        assert!(pending.request(&pick));
    }

    /// A window told it has no dialog answers every pick Unavailable at once, with its own code,
    /// and no COM; one that found the dialog unavailable keeps that code.
    #[test]
    fn a_window_without_the_dialog_answers_unavailable_at_once() {
        let pending = Pending::default();
        pending.refuse();
        let pick = Pick {
            mode: PickMode::Folder,
            title: "t",
            start: None,
        };
        let refused = OsError::from_win32(ERROR_NOT_SUPPORTED, "WindowCx::without_file_dialog");
        for _ in 0..2 {
            assert!(pending.request(&pick));
            assert_eq!(
                pending.run(std::ptr::null_mut()),
                Some(Err(PickError::Unavailable(refused)))
            );
        }
        let found = OsError::from_win32(ERROR_CANCELLED, "test");
        let pending = Pending::default();
        pending.unavailable.set(Some(found));
        pending.refuse();
        assert_eq!(pending.unavailable.get(), Some(found));
    }

    #[test]
    fn a_request_while_a_dialog_is_up_is_refused() {
        let pending = Pending::default();
        let pick = Pick {
            mode: PickMode::Folder,
            title: "t",
            start: None,
        };
        pending.picking.set(true);
        assert!(
            !pending.request(&pick),
            "the dialog's modal loop asks again"
        );
        assert!(pending.run(std::ptr::null_mut()).is_none(), "nothing kept");
        pending.picking.set(false);
        assert!(pending.request(&pick));
    }

    #[test]
    fn show_s_answers_map_to_a_pick_a_cancel_or_unavailable() {
        assert_eq!(shown(0), Ok(true));
        assert_eq!(shown(1), Ok(true), "S_FALSE is a success");
        let cancelled = OsError::from_win32(ERROR_CANCELLED, "IFileDialog::Show");
        assert_eq!(shown(cancelled.code), Ok(false));
        let failed = OsError::from_win32(ERROR_INVALID_PARAMETER, "IFileDialog::Show");
        assert_eq!(shown(failed.code), Err(PickError::Unavailable(failed)));
        let unexpected = hresult(E_UNEXPECTED, "IFileDialog::Show");
        assert_eq!(shown(E_UNEXPECTED), Err(PickError::Unavailable(unexpected)));
    }

    #[test]
    fn the_drive_list_splits_into_roots_in_letter_order() {
        let list: Vec<u16> = "D:\\\0C:\\\0Z:\\\0\0".encode_utf16().collect();
        assert_eq!(
            roots(&list),
            [
                PathBuf::from("C:\\"),
                PathBuf::from("D:\\"),
                PathBuf::from("Z:\\")
            ]
        );
        assert_eq!(roots(&[]), Vec::<PathBuf>::new());
        assert_eq!(roots(&[0]), Vec::<PathBuf>::new());
    }

    #[test]
    fn drives_lists_the_system_drive() {
        let drives = drives();
        let system = std::env::var_os("SystemDrive").map(|mut d| {
            d.push("\\");
            PathBuf::from(d)
        });
        assert!(
            system.is_some_and(|s| drives
                .iter()
                .any(|d| d.as_os_str().eq_ignore_ascii_case(s.as_os_str()))),
            "{drives:?}"
        );
        assert!(drives.windows(2).all(|w| w[0] < w[1]), "in letter order");
    }

    /// A helper thread that finds the window titled `title` (the dialog) and presses Esc in it,
    /// as a user would. It looks for 15 s at most, or until [`Closer::finish`].
    pub(crate) fn close_with_esc(title: &'static str) -> Closer {
        close_with_esc_after(title, Duration::ZERO)
    }

    /// As [`close_with_esc`], `after` once the dialog shows: the window's messages meanwhile go
    /// through the dialog's modal loop.
    pub(crate) fn close_with_esc_after(title: &'static str, after: Duration) -> Closer {
        when_shown(title, move |dialog| {
            thread::sleep(after);
            // SAFETY: posting needs no access to the window; a closed one fails the call.
            unsafe {
                PostMessageW(dialog, WM_KEYDOWN, usize::from(VK_ESCAPE), 0);
                PostMessageW(dialog, WM_KEYUP, usize::from(VK_ESCAPE), 0);
            }
        })
    }

    /// As [`close_with_esc`], but closes the dialog's owner instead (`WM_CLOSE`, as the user's
    /// Alt+F4 or a shutdown would), which takes the dialog with it.
    pub(crate) fn close_owner_when_shown(title: &'static str, owner: HWND) -> Closer {
        // A window handle is a number another thread may post to.
        let owner = owner.addr();
        when_shown(title, move |_| {
            let owner: HWND = std::ptr::without_provenance_mut(owner);
            // SAFETY: as above.
            unsafe { PostMessageW(owner, WM_CLOSE, 0, 0) };
        })
    }

    /// A helper thread that finds the window titled `title` and runs `act` on it once; see
    /// [`close_with_esc`].
    fn when_shown(title: &'static str, act: impl FnOnce(HWND) + Send + 'static) -> Closer {
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            let wide = WideCString::new(title).unwrap_or_else(|e| unreachable!("{e}"));
            let until = Instant::now() + Duration::from_secs(15);
            while Instant::now() < until && !stopped.load(Ordering::Acquire) {
                // SAFETY: a NUL-terminated title, read during the call; no class.
                let dialog = unsafe { FindWindowW(std::ptr::null(), wide.as_ptr()) };
                if !dialog.is_null() {
                    act(dialog);
                    return true;
                }
                thread::sleep(Duration::from_millis(20));
            }
            false
        });
        Closer { stop, thread }
    }

    /// The helper thread of [`close_with_esc`].
    pub(crate) struct Closer {
        stop: Arc<AtomicBool>,
        thread: thread::JoinHandle<bool>,
    }

    impl Closer {
        /// Once the pick returned: stops looking, and says whether it found the dialog.
        pub(crate) fn finish(self) -> bool {
            self.stop.store(true, Ordering::Release);
            self.thread.join().unwrap_or(false)
        }
    }
}
