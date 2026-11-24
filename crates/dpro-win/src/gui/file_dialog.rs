//! The system's file dialog (TD-0009 A3.5 (12)): `IFileOpenDialog`, choosing a folder or a file,
//! owned by the main window, with the in-app picker behind it where Windows has none.
//!
//! - **COM, at run time.** `ole32.dll` is loaded when a dialog is first wanted
//!   (`CoInitializeEx`, `CoCreateInstance`, `CoUninitialize`, `CoTaskMemFree`), so the executable
//!   imports neither `shell32.dll` nor `comdlg32.dll`, and a `WinPE` without them still starts.
//!   Each [`pick`] runs in a [`ComApartment`] of its own on the UI thread, a single-threaded one
//!   entered and left around the call: TD-0009 §5.3's process-long apartment is amended to this.
//!   A thread already in the multithreaded apartment (`RPC_E_CHANGED_MODE`) can't host the
//!   dialog: [`PickError::Unavailable`].
//! - **The dialog.** `CLSID_FileOpenDialog` in process, with `FOS_FORCEFILESYSTEM`,
//!   `FOS_PATHMUSTEXIST`, `FOS_NOCHANGEDIR` and `FOS_DONTADDTORECENT`, and `FOS_PICKFOLDERS` to
//!   choose a folder; a file's types through `SetFileTypes`, the start through `SetDefaultFolder`.
//!   `Show` returning `ERROR_CANCELLED` is `Ok(None)`; otherwise the result's
//!   `SIGDN_FILESYSPATH`, freed with `CoTaskMemFree`.
//! - **Unavailable.** The class not registered, or `Show` failing with anything but a cancel, is
//!   [`PickError::Unavailable`]: the window remembers it for the session, answering every later
//!   request with it at once, and the model opens the in-app picker instead (A3.10 item 9: no
//!   setting).
//! - **Never inside an event.** `Show` runs a modal loop, and a handler must not pump messages.
//!   So the handler asks with [`WindowCx::pick`](super::WindowCx::pick), the window runs the
//!   dialog once the handler has returned and nothing of it is borrowed, and the answer comes
//!   back through [`Handler::picked`](super::Handler::picked). The modal loop still dispatches
//!   the window's paints, timers and wakes meanwhile. A5's `Effect::Pick` replaces the request.
//!
//! [`drives`] lists the drive roots, for the in-app picker's drive list.
//!
//! Slice 6e's MEASURE stub: [`pick`] and [`ComApartment::sta`] answer
//! [`PickError::Unavailable`] without loading COM, and [`drives`] lists none, so a model's request
//! comes back Unavailable and the in-app picker opens. The tests that need the code are ignored
//! until it lands.

use std::{
    cell::{Cell, RefCell},
    marker::PhantomData,
    path::{Path, PathBuf},
};

use dpro_core::OsError;
use windows_sys::Win32::Foundation::{ERROR_CALL_NOT_IMPLEMENTED, HWND};

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
    /// The dialog ran and failed (its result had no file-system path).
    #[error("the system file dialog failed: {0}")]
    Failed(OsError),
}

/// A pick's answer: the path, or `None` if the user cancelled.
pub type Picked = Result<Option<PathBuf>, PickError>;

/// Shows the system dialog, modal to `owner`, and waits for the user. `Ok(None)` if they
/// cancelled. Never call it while handling an event: ask with
/// [`WindowCx::pick`](super::WindowCx::pick).
///
/// # Errors
/// [`PickError::Unavailable`] where there's no dialog; [`PickError::Failed`] if it failed.
pub fn pick(owner: Hwnd, pick: &Pick<'_>) -> Picked {
    let _ = (owner, pick);
    Err(PickError::Unavailable(not_yet()))
}

/// The drive roots (`C:\`, `D:\`), in letter order, for the in-app picker's drive list; empty if
/// Windows wouldn't say (`GetLogicalDriveStringsW`).
#[must_use]
pub fn drives() -> Vec<PathBuf> {
    Vec::new()
}

/// A single-threaded COM apartment on this thread, entered for one dialog: `CoInitializeEx`
/// with `COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE`, balanced by `CoUninitialize` when
/// it's dropped, on the same thread (it's neither `Send` nor `Sync`). Entering it again while it
/// holds (`S_FALSE`) is fine: each balances its own.
#[derive(Debug)]
pub struct ComApartment {
    _thread: PhantomData<*const ()>,
}

impl ComApartment {
    /// Enters the apartment.
    ///
    /// # Errors
    /// [`PickError::Unavailable`]: `ole32.dll` isn't there, or this thread is in the
    /// multithreaded apartment (`RPC_E_CHANGED_MODE`).
    pub fn sta() -> Result<Self, PickError> {
        Err(PickError::Unavailable(not_yet()))
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {}
}

/// The stub's answer until the dialog lands.
fn not_yet() -> OsError {
    OsError::from_win32(ERROR_CALL_NOT_IMPLEMENTED, "IFileOpenDialog")
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

    /// Once the handler has returned: runs the kept pick, modal to `owner`, and returns its
    /// answer; `None` if none was kept.
    pub(crate) fn run(&self, owner: HWND) -> Option<Picked> {
        let request = self.request.try_borrow_mut().ok()?.take()?;
        let types: Vec<(&str, &str)> = request.types.iter().map(|(l, p)| (&**l, &**p)).collect();
        let mode = if request.folder {
            PickMode::Folder
        } else {
            PickMode::File { types: &types }
        };
        let _ = (&self.unavailable, &self.picking);
        Some(pick(
            Hwnd(owner),
            &Pick {
                mode,
                title: &request.title,
                start: request.start.as_deref(),
            },
        ))
    }
}

/// T2 runs these under Wine; T3 and G1 open the dialog on Windows (`examples/file_dialog_t3.rs`).
/// Each COM test runs on a thread of its own, whose apartment nothing else touched. The window's
/// side (a pick after the event, its answer through `Handler::picked`) is tested in `window.rs`.
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
            WindowsAndMessaging::{FindWindowW, PostMessageW, WM_KEYDOWN, WM_KEYUP},
        },
        core::HRESULT,
    };

    use super::*;
    use crate::{
        dll::{self, Proc},
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
    #[ignore = "slice 6e: lands with the code"]
    fn the_apartment_balances_its_enters() {
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
    #[ignore = "slice 6e: lands with the code"]
    fn a_thread_in_the_multithreaded_apartment_has_no_dialog() {
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
    #[ignore = "slice 6e: lands with the code"]
    fn the_dialog_is_made_or_recorded_unavailable() {
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
    #[ignore = "slice 6e: lands with the code"]
    fn esc_in_the_dialog_is_a_cancel() {
        let picked = on_thread(|| {
            let title = "dpro-win file_dialog test: esc";
            let closer = close_with_esc(title);
            let types = [("Windows images", "*.wim;*.esd"), ("All files", "*.*")];
            let picked = pick(
                Hwnd::NONE,
                &Pick {
                    mode: PickMode::File { types: &types },
                    title,
                    start: None,
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

    #[test]
    #[ignore = "slice 6e: lands with the code"]
    fn a_window_remembers_the_dialog_is_unavailable() {
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
        let pick = Pick {
            mode: PickMode::Folder,
            title: "t",
            start: None,
        };
        assert!(pending.request(&pick));
        assert!(!pending.request(&pick), "one pick at a time");
        assert!(pending.run(std::ptr::null_mut()).is_some());
        assert!(pending.run(std::ptr::null_mut()).is_none(), "taken once");
        assert!(pending.request(&pick));
    }

    #[test]
    #[ignore = "slice 6e: lands with the code"]
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
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            let wide = WideCString::new(title).unwrap_or_else(|e| unreachable!("{e}"));
            let until = Instant::now() + Duration::from_secs(15);
            while Instant::now() < until && !stopped.load(Ordering::Acquire) {
                // SAFETY: a NUL-terminated title, read during the call; no class.
                let dialog = unsafe { FindWindowW(std::ptr::null(), wide.as_ptr()) };
                if !dialog.is_null() {
                    thread::sleep(after);
                    // SAFETY: posting needs no access to the window; a closed one fails the call.
                    unsafe {
                        PostMessageW(dialog, WM_KEYDOWN, usize::from(VK_ESCAPE), 0);
                        PostMessageW(dialog, WM_KEYUP, usize::from(VK_ESCAPE), 0);
                    }
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
