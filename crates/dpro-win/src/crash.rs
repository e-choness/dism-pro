//! Local crash dumps (roadmap 2.1, coding standards §3): a panic or an unhandled exception leaves
//! a minidump in `<state>\crash`. Nothing leaves the machine, and the folder is admin-only because
//! a dump holds whatever was on the stack. Without elevation there's no folder to write to, so
//! the crash just ends the process as it would have.
//!
//! `dbghelp.dll` is loaded when a dump is written, not imported: the exe must start where it's
//! missing (`WinPE`, `xtask pe-compat`), and only System32's copy may ever load.

use std::{
    ffi::c_void,
    fs::{self, File},
    os::windows::io::AsRawHandle,
    panic,
    path::{Path, PathBuf},
    ptr,
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use windows_sys::{
    Win32::{
        Foundation::HANDLE,
        System::{
            Diagnostics::Debug::{
                EXCEPTION_POINTERS, MINIDUMP_EXCEPTION_INFORMATION, MINIDUMP_TYPE,
                MiniDumpWithThreadInfo, SetUnhandledExceptionFilter,
            },
            LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW},
            Threading::{GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId},
        },
    },
    core::BOOL,
};

use crate::{security, wide::WideCString};

/// Dumps kept per machine: a tool crashing in a script's loop must not fill the disk.
const KEEP: usize = 16;

/// Ends the process from the exception filter, without Windows Error Reporting.
const EXECUTE_HANDLER: i32 = 1;

type MiniDumpWriteDump = unsafe extern "system" fn(
    HANDLE,
    u32,
    HANDLE,
    MINIDUMP_TYPE,
    *const MINIDUMP_EXCEPTION_INFORMATION,
    *const c_void,
    *const c_void,
) -> BOOL;

type Notice = Box<dyn Fn(&Path) + Send + Sync>;

struct Config {
    state_dir: PathBuf,
    notice: Notice,
}

static CONFIG: OnceLock<Config> = OnceLock::new();

/// One dump per process, however many threads go down.
static WRITING: AtomicBool = AtomicBool::new(false);

/// Installs the panic hook and the exception filter. `state_dir` is the admin-only state folder,
/// and `notice` hears the path of a dump that was written. Only the first call has an effect.
pub fn install(state_dir: PathBuf, notice: impl Fn(&Path) + Send + Sync + 'static) {
    let config = Config {
        state_dir,
        notice: Box::new(notice),
    };
    if CONFIG.set(config).is_err() {
        return;
    }
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        previous(info);
        // MiniDumpWriteDump can't walk the stack of the thread calling it.
        // ignore: the process is going down either way.
        let _ = thread::spawn(|| write(None)).join();
    }));
    // SAFETY: `on_exception` is a valid filter for the life of the process.
    unsafe { SetUnhandledExceptionFilter(Some(on_exception)) };
}

unsafe extern "system" fn on_exception(pointers: *const EXCEPTION_POINTERS) -> i32 {
    write(Some(pointers));
    EXECUTE_HANDLER
}

fn write(pointers: Option<*const EXCEPTION_POINTERS>) {
    let Some(config) = CONFIG.get() else {
        return;
    };
    if WRITING.swap(true, Ordering::AcqRel) {
        return;
    }
    let dir = config.state_dir.join("crash");
    if security::ensure_admin_only_dir(&config.state_dir).is_err()
        || fs::create_dir_all(&dir).is_err()
    {
        return;
    }
    let path = dir.join(file_name());
    if dump(&path, pointers).is_some() {
        (config.notice)(&path);
        prune(&dir);
    } else {
        // ignore: a failed dump may leave an empty file, or none.
        let _ = fs::remove_file(&path);
    }
}

fn file_name() -> String {
    let exe = std::env::current_exe().ok();
    let stem = exe
        .as_deref()
        .and_then(Path::file_stem)
        .map_or("dismpro".into(), |s| s.to_string_lossy());
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |t| t.as_secs());
    format!("{stem}-{seconds}-{}.dmp", std::process::id())
}

fn dump(path: &Path, pointers: Option<*const EXCEPTION_POINTERS>) -> Option<()> {
    let name = WideCString::new("dbghelp.dll").ok()?;
    // SAFETY: `name` is NUL-terminated; a null file handle is what the call expects.
    let module =
        unsafe { LoadLibraryExW(name.as_ptr(), ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32) };
    if module.is_null() {
        return None;
    }
    // SAFETY: `module` is loaded (and stays loaded); the name is NUL-terminated.
    let entry = unsafe { GetProcAddress(module, c"MiniDumpWriteDump".as_ptr().cast()) }?;
    // SAFETY: the export has this signature (dbghelp.h).
    let write_dump: MiniDumpWriteDump = unsafe { std::mem::transmute(entry) };

    let file = File::create_new(path).ok()?;
    let info = pointers.map(|pointers| MINIDUMP_EXCEPTION_INFORMATION {
        // SAFETY: no preconditions.
        ThreadId: unsafe { GetCurrentThreadId() },
        ExceptionPointers: pointers.cast_mut(),
        ClientPointers: 0,
    });
    // SAFETY: pseudo-handle and id of this process; `file` is open for writing; `info` is either
    // null or lives across the call, and its pointers came from the OS for this exception.
    let written = unsafe {
        write_dump(
            GetCurrentProcess(),
            GetCurrentProcessId(),
            file.as_raw_handle(),
            MiniDumpWithThreadInfo,
            info.as_ref().map_or(ptr::null(), ptr::from_ref),
            ptr::null(),
            ptr::null(),
        )
    };
    (written != 0).then_some(())
}

/// The files past the newest `keep`, oldest first.
fn surplus(mut files: Vec<(SystemTime, PathBuf)>, keep: usize) -> Vec<PathBuf> {
    files.sort();
    let excess = files.len().saturating_sub(keep);
    files.into_iter().take(excess).map(|(_, p)| p).collect()
}

fn prune(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let dumps = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "dmp"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    for path in surplus(dumps, KEEP) {
        // ignore: the dump that matters is written, and the next crash tries again.
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use std::{process::Command, time::Duration};

    use windows_sys::Win32::System::Diagnostics::Debug::RaiseException;

    use super::*;

    const DIR: &str = "DPRO_CRASH_TEST_DIR";
    const MODE: &str = "DPRO_CRASH_TEST_MODE";

    /// What the tests below crash, in a child process of this test binary.
    #[test]
    #[ignore = "a child of the crash tests"]
    fn crash_child() {
        let dir = PathBuf::from(std::env::var_os(DIR).unwrap());
        let notes = dir.join("notice.txt");
        install(dir, move |path| {
            fs::write(&notes, path.display().to_string()).unwrap();
        });
        match std::env::var(MODE).as_deref() {
            Ok("panic") => panic!("expected"),
            // SAFETY: a software exception without arguments; flag 1 is noncontinuable.
            _ => unsafe { RaiseException(0xE000_0001, 1, 0, ptr::null()) },
        }
    }

    fn crash(name: &str, mode: &str) -> Vec<u8> {
        let dir = std::env::temp_dir().join(format!("dpro-crash-{name}-{}", std::process::id()));
        // ignore: left over from an earlier run, or nothing there.
        let _ = fs::remove_dir_all(&dir);
        // Captured: the child's deliberate crash stays out of the test log unless a check fails.
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "crash::tests::crash_child"])
            .env(DIR, &dir)
            .env(MODE, mode)
            .output()
            .unwrap();
        let log = format!(
            "child stdout:\n{}\nchild stderr:\n{}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
        assert!(!child.status.success(), "the child must not survive\n{log}");
        let dumps: Vec<PathBuf> = fs::read_dir(dir.join("crash"))
            .map(|d| d.map(|e| e.unwrap().path()).collect())
            .unwrap_or_default();
        assert_eq!(dumps.len(), 1, "{dumps:?}\n{log}");
        let noticed = fs::read_to_string(dir.join("notice.txt")).unwrap();
        assert_eq!(noticed, dumps[0].display().to_string());
        let dump = fs::read(&dumps[0]).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        dump
    }

    #[test]
    fn a_panic_leaves_a_dump() {
        assert!(crash("panic", "panic").starts_with(b"MDMP"));
    }

    #[test]
    fn an_unhandled_exception_leaves_a_dump() {
        assert!(crash("seh", "exception").starts_with(b"MDMP"));
    }

    #[test]
    fn only_the_oldest_files_go() {
        let at = |s: u64| UNIX_EPOCH + Duration::from_secs(s);
        let files = vec![
            (at(3), PathBuf::from("c")),
            (at(1), PathBuf::from("a")),
            (at(2), PathBuf::from("b")),
        ];
        assert_eq!(surplus(files.clone(), 1), ["a", "b"].map(PathBuf::from));
        assert_eq!(surplus(files.clone(), 3), Vec::<PathBuf>::new());
        assert_eq!(surplus(files, 9), Vec::<PathBuf>::new());
    }
}
