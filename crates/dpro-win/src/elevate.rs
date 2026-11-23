//! Starting a process elevated, through the UAC prompt (ADR-0006, TD-0013 §15). Windows shows
//! the prompt and the user decides; nothing here bypasses it.

use std::{
    borrow::Cow,
    ffi::c_void,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    path::Path,
    ptr,
};

use windows_sys::{
    Win32::{
        Foundation::{
            ERROR_CANCELLED, ERROR_NO_ASSOCIATION, GetLastError, WAIT_OBJECT_0, WAIT_TIMEOUT,
        },
        System::Threading::{GetExitCodeProcess, WaitForSingleObject},
        UI::Shell::{
            SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
        },
    },
    core::{BOOL, HRESULT},
};

use dpro_core::OsError;

use crate::{
    dll::{self, Proc},
    error::{Result, last_error_if_false, last_os_error},
    wide::WideCString,
};

/// `SW_HIDE`: the broker is headless, so its console window stays hidden.
const SW_HIDE: i32 = 0;
/// `COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE`, as `ShellExecuteExW` asks of its caller.
const COINIT_STA_NO_DDE: u32 = 0x2 | 0x4;

// shell32 and ole32 are loaded only when a process is elevated (the MCP front starting its
// broker): every other run of the CLI, and WinPE, never load them.
type ShellExecuteExW = unsafe extern "system" fn(*mut SHELLEXECUTEINFOW) -> BOOL;
type CoInitializeEx = unsafe extern "system" fn(*const c_void, u32) -> HRESULT;
type CoUninitialize = unsafe extern "system" fn();

/// What became of a request to run elevated.
#[derive(Debug)]
pub enum Elevated {
    /// The process started.
    Started(Child),
    /// The user dismissed the UAC prompt.
    Declined,
}

/// A process started by [`run_elevated`]: enough to notice that it exited, and how.
#[derive(Debug)]
pub struct Child(OwnedHandle);

impl Child {
    /// `Some(exit code)` once the process has exited, `None` while it runs.
    ///
    /// # Errors
    /// The OS error from waiting on it or reading its exit code.
    pub fn exit_code(&self) -> Result<Option<u32>> {
        // SAFETY: a valid process handle; a zero timeout only polls.
        let wait = unsafe { WaitForSingleObject(self.0.as_raw_handle(), 0) };
        if wait == WAIT_TIMEOUT {
            return Ok(None);
        }
        if wait != WAIT_OBJECT_0 {
            return Err(last_os_error("WaitForSingleObject").into());
        }
        let mut code = 0_u32;
        // SAFETY: a valid process handle; `code` is a valid out pointer.
        let ok = unsafe { GetExitCodeProcess(self.0.as_raw_handle(), &raw mut code) };
        last_error_if_false(ok, "GetExitCodeProcess")?;
        Ok(Some(code))
    }
}

/// Runs `exe` with `args` elevated and hidden, via `ShellExecuteExW` with the `runas` verb.
/// Blocks until the user answers the UAC prompt (immediately where UAC is off). The call runs on
/// a thread of its own with COM initialized, as `ShellExecuteExW` requires.
///
/// # Errors
/// Any failure other than the user declining, which is [`Elevated::Declined`].
pub fn run_elevated(exe: &Path, args: &[&str]) -> Result<Elevated> {
    let verb = WideCString::new("runas")?;
    let file = WideCString::new(exe)?;
    let params = WideCString::new(command_line(args))?;
    std::thread::scope(
        |s| match s.spawn(|| shell_execute(&verb, &file, &params)).join() {
            Ok(result) => result,
            Err(panic) => std::panic::resume_unwind(panic),
        },
    )
}

fn shell_execute(verb: &WideCString, file: &WideCString, params: &WideCString) -> Result<Elevated> {
    let ole32 = dll::system("ole32.dll")?;
    let shell32 = dll::system("shell32.dll")?;
    // SAFETY: the exports have these signatures (objbase.h, shellapi.h).
    let (co_initialize, co_uninitialize, shell_execute_ex) = unsafe {
        (
            std::mem::transmute::<Proc, CoInitializeEx>(dll::export(ole32, c"CoInitializeEx")?),
            std::mem::transmute::<Proc, CoUninitialize>(dll::export(ole32, c"CoUninitialize")?),
            std::mem::transmute::<Proc, ShellExecuteExW>(dll::export(shell32, c"ShellExecuteExW")?),
        )
    };
    // SAFETY: no preconditions; a success (S_OK or S_FALSE) is balanced by CoUninitialize below,
    // on this same thread.
    let com = unsafe { co_initialize(ptr::null(), COINIT_STA_NO_DDE) } >= 0;
    let mut info = SHELLEXECUTEINFOW {
        cbSize: u32::try_from(size_of::<SHELLEXECUTEINFOW>()).unwrap_or(u32::MAX),
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: verb.as_ptr(),
        lpFile: file.as_ptr(),
        lpParameters: params.as_ptr(),
        nShow: SW_HIDE,
        ..Default::default()
    };
    // SAFETY: `info` has its size set; the strings are NUL-terminated and outlive the call.
    let ok = unsafe { shell_execute_ex(&raw mut info) };
    // SAFETY: no preconditions; read before anything else can change it.
    let code = unsafe { GetLastError() };
    let result = if ok == 0 {
        if code == ERROR_CANCELLED {
            Ok(Elevated::Declined)
        } else {
            Err(OsError::from_win32(code, "ShellExecuteExW").into())
        }
    } else if info.hProcess.is_null() {
        // Only for documents handed to another program; an .exe always gets a process.
        Err(OsError::from_win32(ERROR_NO_ASSOCIATION, "ShellExecuteExW").into())
    } else {
        // SAFETY: SEE_MASK_NOCLOSEPROCESS hands the new process's handle to the caller, who
        // owns it from here.
        Ok(Elevated::Started(Child(unsafe {
            OwnedHandle::from_raw_handle(info.hProcess)
        })))
    };
    if com {
        // SAFETY: balances the successful CoInitializeEx above, on the same thread.
        unsafe { co_uninitialize() };
    }
    result
}

/// Arguments joined into one command line that `CommandLineToArgvW` (and Rust's `args()`)
/// splits back into the same strings.
fn command_line(args: &[&str]) -> String {
    args.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ")
}

fn quote(arg: &str) -> Cow<'_, str> {
    if !arg.is_empty() && !arg.contains([' ', '\t', '\n', '\x0b', '"']) {
        return Cow::Borrowed(arg);
    }
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('"');
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                // Backslashes before a quote are doubled, and the quote itself escaped.
                out.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.extend(std::iter::repeat_n('\\', backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    // Backslashes before the closing quote are doubled, so it stays a closing quote.
    out.extend(std::iter::repeat_n('\\', backslashes * 2));
    out.push('"');
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    /// Started by the test below; the test harness exits 0 after it.
    #[test]
    #[ignore = "a child of elevate::tests::an_elevated_caller_starts_a_child_without_a_prompt"]
    fn elevated_child() {}

    /// An already elevated caller gets no UAC prompt, so this runs on T3 (the runner's tests run
    /// elevated) and is skipped where it would prompt someone.
    #[test]
    fn an_elevated_caller_starts_a_child_without_a_prompt() {
        if !crate::privilege::is_elevated().unwrap_or(false) {
            return;
        }
        let exe = std::env::current_exe().unwrap();
        let args = ["--exact", "elevate::tests::elevated_child", "--ignored"];
        let Elevated::Started(child) = run_elevated(&exe, &args).unwrap() else {
            panic!("declined while already elevated");
        };
        let deadline = Instant::now() + Duration::from_secs(60);
        let code = loop {
            if let Some(code) = child.exit_code().unwrap() {
                break code;
            }
            assert!(Instant::now() < deadline, "the child didn't exit");
            std::thread::sleep(Duration::from_millis(50));
        };
        assert_eq!(code, 0);
    }

    #[test]
    fn quoting_follows_the_command_line_rules() {
        let cases = [
            ("--agent-broker", "--agent-broker"),
            ("S-1-5-21-1-2-3-1001", "S-1-5-21-1-2-3-1001"),
            ("", r#""""#),
            ("two words", r#""two words""#),
            (r"C:\Program Files\", r#""C:\Program Files\\""#),
            (r#"say "hi""#, r#""say \"hi\"""#),
            (r#"a\"b"#, r#""a\\\"b""#),
            (r"no\space", r"no\space"),
        ];
        for (arg, want) in cases {
            assert_eq!(quote(arg), want, "{arg}");
        }
        assert_eq!(
            command_line(&["--agent-broker", "--user-sid", "S-1-5-18"]),
            "--agent-broker --user-sid S-1-5-18"
        );
    }
}
