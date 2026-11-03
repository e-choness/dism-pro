//! The named pipe between the unelevated MCP front and the elevated agent broker (ADR-0006,
//! TD-0013 §5.1, §15).
//!
//! - Message mode, local clients only. The first instance is created with
//!   `FILE_FLAG_FIRST_PIPE_INSTANCE`, so a squatter's earlier pipe makes [`Pipe::create`] fail
//!   instead of sharing the name. The broker adds instances ([`Pipe::next_instance`]) to serve
//!   several fronts.
//! - The DACL gives one user SID exactly what a client end needs ([`CLIENT_ACCESS`]), and
//!   Administrators full access. The explicit Medium mandatory label lets that user's
//!   *unelevated* front open a pipe that an elevated broker created. The user doesn't get
//!   `FILE_CREATE_PIPE_INSTANCE`, so a process of the same user can't add an instance of its own
//!   and catch a front's connection.
//! - The client end allows only identification-level impersonation, so a server that isn't the
//!   broker can't act as the front.
//! - Peers check each other with [`Pipe::peer_pid`], [`process_image`] and [`sha256_file`].

use std::{
    ffi::{OsString, c_void},
    os::windows::{
        ffi::OsStringExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
    ptr,
};

use windows_sys::Win32::{
    Foundation::{
        ERROR_MORE_DATA, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GetLastError, HANDLE,
        INVALID_HANDLE_VALUE,
    },
    Security::{
        Authorization::ConvertSidToStringSidW,
        Cryptography::{BCRYPT_SHA256_ALG_HANDLE, BCryptHash},
        GetTokenInformation, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenSessionId,
        TokenUser,
    },
    Storage::FileSystem::{
        CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_READ_ATTRIBUTES, FILE_READ_DATA,
        FILE_WRITE_ATTRIBUTES, FILE_WRITE_DATA, OPEN_EXISTING, PIPE_ACCESS_DUPLEX, ReadFile,
        SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT, SYNCHRONIZE, WriteFile,
    },
    System::{
        Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId,
            GetNamedPipeServerProcessId, PIPE_READMODE_MESSAGE, PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_TYPE_MESSAGE, PIPE_WAIT, SetNamedPipeHandleState, WaitNamedPipeW,
        },
        Threading::{
            GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
        },
    },
};

use dpro_core::OsError;

use crate::{
    error::{Error, Result, last_error_if_false, last_os_error},
    security::{LocalBox, descriptor, io_error},
    wide::{WideCString, from_wide_ptr},
};

/// The largest message either side accepts. Tool results stay under 32 KiB (TD-0013 §5.6); the
/// rest is headroom for plans, which are paged.
pub const MAX_MESSAGE: usize = 1 << 20;
/// The pipe's own buffers, each way: the 64 KiB round-trip budget (TD-0013 §7) in one write.
const BUFFER: u32 = 64 * 1024;
/// Read chunk for message-mode reads.
const CHUNK: usize = 64 * 1024;
/// Fronts one broker serves at once; a further front waits for a free instance.
pub const MAX_INSTANCES: u32 = 8;
/// What a client end needs and no more: read and write data, read attributes (the server's
/// pid), write attributes (message read mode), wait. Not `FILE_CREATE_PIPE_INSTANCE`.
pub const CLIENT_ACCESS: u32 =
    FILE_READ_DATA | FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES | SYNCHRONIZE;

/// One end of the broker pipe.
#[derive(Debug)]
pub struct Pipe {
    handle: OwnedHandle,
    /// Server ends keep what [`Pipe::next_instance`] needs.
    server: Option<Instance>,
}

#[derive(Debug, Clone)]
struct Instance {
    path: WideCString,
    sddl: String,
}

impl Pipe {
    /// Server end: creates `\\.\pipe\<name>` as its first instance, open to the user `sid`
    /// (`S-1-…`) at medium integrity with [`CLIENT_ACCESS`], to Administrators, and to no one
    /// else.
    ///
    /// # Errors
    /// A name or SID that isn't plain ([`Error::InvalidName`]); `ERROR_ACCESS_DENIED` when the
    /// name is already taken, which callers must treat as possible squatting.
    pub fn create(name: &str, sid: &str) -> Result<Self> {
        if !is_sid(sid) {
            return Err(Error::InvalidName(sid.to_owned()));
        }
        let instance = Instance {
            path: pipe_path(name)?,
            // TD-0013 §15; the label is medium, no write up.
            sddl: format!("D:P(A;;GA;;;BA)(A;;{CLIENT_ACCESS:#x};;;{sid})S:(ML;;NW;;;ME)"),
        };
        Self::listen(instance, FILE_FLAG_FIRST_PIPE_INSTANCE)
    }

    /// Server end: one more instance of this pipe, for the next front, while this one serves
    /// its client. Needs the right to create instances, which only Administrators have.
    ///
    /// # Errors
    /// [`Error::ClientEnd`] on a client end; `ERROR_PIPE_BUSY` past [`MAX_INSTANCES`]; other
    /// OS errors.
    pub fn next_instance(&self) -> Result<Self> {
        let instance = self.server.clone().ok_or(Error::ClientEnd)?;
        Self::listen(instance, 0)
    }

    fn listen(instance: Instance, first: u32) -> Result<Self> {
        let sd = descriptor(&instance.sddl)?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(u32::MAX),
            lpSecurityDescriptor: sd.0,
            bInheritHandle: 0,
        };
        // SAFETY: the path is NUL-terminated; `attributes` and the descriptor it points to live
        // until after the call.
        let handle = unsafe {
            CreateNamedPipeW(
                instance.path.as_ptr(),
                PIPE_ACCESS_DUPLEX | first,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                MAX_INSTANCES,
                BUFFER,
                BUFFER,
                0,
                &raw const attributes,
            )
        };
        Ok(Self {
            handle: owned(handle, "CreateNamedPipeW")?,
            server: Some(instance),
        })
    }

    /// Server end: waits for the client to connect.
    ///
    /// # Errors
    /// OS errors.
    pub fn accept(&self) -> Result<()> {
        // SAFETY: the handle is a pipe server handle opened without FILE_FLAG_OVERLAPPED, so a
        // null OVERLAPPED is valid.
        if unsafe { ConnectNamedPipe(self.raw(), ptr::null_mut()) } != 0 {
            return Ok(());
        }
        // SAFETY: no preconditions; read right after the failing call.
        match unsafe { GetLastError() } {
            // The client connected between create and connect.
            ERROR_PIPE_CONNECTED => Ok(()),
            code => Err(OsError::from_win32(code, "ConnectNamedPipe").into()),
        }
    }

    /// Client end: opens `\\.\pipe\<name>`, waiting up to `timeout_ms` while it is busy. The
    /// server may identify this process but not impersonate it.
    ///
    /// # Errors
    /// [`Error::InvalidName`]; `ERROR_FILE_NOT_FOUND` when no server runs; other OS errors.
    pub fn connect(name: &str, timeout_ms: u32) -> Result<Self> {
        let path = pipe_path(name)?;
        let mut waited = false;
        loop {
            // SAFETY: `path` is NUL-terminated; no security attributes or template.
            let handle = unsafe {
                CreateFileW(
                    path.as_ptr(),
                    CLIENT_ACCESS,
                    0,
                    ptr::null(),
                    OPEN_EXISTING,
                    SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                    ptr::null_mut(),
                )
            };
            if handle != INVALID_HANDLE_VALUE {
                let pipe = Self {
                    handle: owned(handle, "CreateFileW")?,
                    server: None,
                };
                let mode = PIPE_READMODE_MESSAGE;
                // SAFETY: valid pipe handle; `mode` outlives the call; the other two are optional.
                let ok = unsafe {
                    SetNamedPipeHandleState(pipe.raw(), &raw const mode, ptr::null(), ptr::null())
                };
                last_error_if_false(ok, "SetNamedPipeHandleState")?;
                return Ok(pipe);
            }
            let err = last_os_error("CreateFileW");
            if waited || err.code != hresult(ERROR_PIPE_BUSY) {
                return Err(err.into());
            }
            waited = true;
            // SAFETY: `path` is NUL-terminated.
            let ok = unsafe { WaitNamedPipeW(path.as_ptr(), timeout_ms) };
            last_error_if_false(ok, "WaitNamedPipeW")?;
        }
    }

    /// The process id of the other end: the client's on the server end, the server's on the
    /// client end.
    ///
    /// # Errors
    /// OS errors.
    pub fn peer_pid(&self) -> Result<u32> {
        let mut pid = 0_u32;
        // SAFETY: valid pipe handle; `pid` is a valid out pointer.
        let ok = unsafe {
            if self.server.is_some() {
                GetNamedPipeClientProcessId(self.raw(), &raw mut pid)
            } else {
                GetNamedPipeServerProcessId(self.raw(), &raw mut pid)
            }
        };
        last_error_if_false(ok, "GetNamedPipe*ProcessId")?;
        Ok(pid)
    }

    /// Sends one message.
    ///
    /// # Errors
    /// [`Error::MessageTooLarge`] above [`MAX_MESSAGE`]; OS errors (a closed peer is
    /// `ERROR_NO_DATA` or `ERROR_BROKEN_PIPE`).
    pub fn send(&self, message: &[u8]) -> Result<()> {
        if message.len() > MAX_MESSAGE {
            return Err(Error::MessageTooLarge(message.len()));
        }
        let len =
            u32::try_from(message.len()).map_err(|_| Error::MessageTooLarge(message.len()))?;
        let mut written = 0_u32;
        // SAFETY: `message` is readable for `len` bytes; synchronous handle, so no OVERLAPPED.
        let ok = unsafe {
            WriteFile(
                self.raw(),
                message.as_ptr(),
                len,
                &raw mut written,
                ptr::null_mut(),
            )
        };
        last_error_if_false(ok, "WriteFile")?;
        if written == len {
            Ok(())
        } else {
            Err(OsError::from_win32(ERROR_MORE_DATA, "WriteFile").into())
        }
    }

    /// Receives one whole message into `buf` (replacing its contents).
    ///
    /// # Errors
    /// [`Error::MessageTooLarge`] above [`MAX_MESSAGE`] (the connection should then be dropped:
    /// the rest of the message is still queued); OS errors (`ERROR_BROKEN_PIPE` when the peer
    /// closed).
    pub fn recv(&self, buf: &mut Vec<u8>) -> Result<()> {
        buf.clear();
        loop {
            let start = buf.len();
            if start >= MAX_MESSAGE {
                return Err(Error::MessageTooLarge(start));
            }
            buf.resize(start + CHUNK, 0);
            let mut read = 0_u32;
            let chunk = u32::try_from(CHUNK).unwrap_or(u32::MAX);
            // SAFETY: `buf[start..]` is writable for CHUNK bytes; synchronous handle.
            let ok = unsafe {
                ReadFile(
                    self.raw(),
                    buf[start..].as_mut_ptr(),
                    chunk,
                    &raw mut read,
                    ptr::null_mut(),
                )
            };
            let code = if ok == 0 {
                // SAFETY: no preconditions; read right after the call.
                unsafe { GetLastError() }
            } else {
                0
            };
            buf.truncate(start + usize::try_from(read).unwrap_or(0));
            match code {
                0 => return Ok(()),
                ERROR_MORE_DATA => {}
                code => return Err(OsError::from_win32(code, "ReadFile").into()),
            }
        }
    }

    fn raw(&self) -> HANDLE {
        self.handle.as_raw_handle()
    }
}

/// The full Win32 path of a running process's image.
///
/// # Errors
/// OS errors (`ERROR_INVALID_PARAMETER` for a process that has exited).
pub fn process_image(pid: u32) -> Result<PathBuf> {
    let process = open_process(pid)?;
    let mut buf = vec![0_u16; 32 * 1024];
    let mut len = u32::try_from(buf.len()).unwrap_or(u32::MAX);
    // SAFETY: valid process handle; `buf` holds `len` units; `len` is a valid in/out pointer.
    let ok = unsafe {
        QueryFullProcessImageNameW(
            process.as_raw_handle(),
            PROCESS_NAME_WIN32,
            buf.as_mut_ptr(),
            &raw mut len,
        )
    };
    last_error_if_false(ok, "QueryFullProcessImageNameW")?;
    buf.truncate(usize::try_from(len).unwrap_or(0));
    Ok(PathBuf::from(OsString::from_wide(&buf)))
}

/// The user SID of this process's token, as `S-1-…`.
///
/// # Errors
/// OS errors.
pub fn current_user_sid() -> Result<String> {
    // SAFETY: GetCurrentProcess returns a pseudo handle that needs no closing.
    token_user_sid(&process_token(unsafe { GetCurrentProcess() })?)
}

/// The user SID of another process's token, as `S-1-…`. An unelevated caller can't read an
/// elevated process's token; the broker reads its fronts'.
///
/// # Errors
/// OS errors (`ERROR_ACCESS_DENIED` for a process this one may not inspect).
pub fn process_user_sid(pid: u32) -> Result<String> {
    let process = open_process(pid)?;
    token_user_sid(&process_token(process.as_raw_handle())?)
}

/// The Remote Desktop session this process runs in (its token's session id).
///
/// # Errors
/// OS errors.
pub fn current_session_id() -> Result<u32> {
    // SAFETY: GetCurrentProcess returns a pseudo handle that needs no closing.
    let token = process_token(unsafe { GetCurrentProcess() })?;
    let mut session = 0_u32;
    let mut needed = 0_u32;
    // SAFETY: valid token handle; `session` is writable for its size; `needed` is valid.
    let ok = unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenSessionId,
            (&raw mut session).cast::<c_void>(),
            u32::try_from(size_of::<u32>()).unwrap_or(0),
            &raw mut needed,
        )
    };
    last_error_if_false(ok, "GetTokenInformation")?;
    Ok(session)
}

fn open_process(pid: u32) -> Result<OwnedHandle> {
    // SAFETY: no pointer arguments.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err(last_os_error("OpenProcess").into());
    }
    // SAFETY: OpenProcess returned a valid handle that nothing else owns.
    Ok(unsafe { OwnedHandle::from_raw_handle(process) })
}

fn process_token(process: HANDLE) -> Result<OwnedHandle> {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: `process` is a valid process handle (or the current-process pseudo handle);
    // `token` is a valid out pointer.
    let ok = unsafe { OpenProcessToken(process, TOKEN_QUERY, &raw mut token) };
    last_error_if_false(ok, "OpenProcessToken")?;
    // SAFETY: OpenProcessToken returned a valid handle that nothing else owns.
    Ok(unsafe { OwnedHandle::from_raw_handle(token) })
}

fn token_user_sid(token: &OwnedHandle) -> Result<String> {
    // TOKEN_USER plus its SID: well under 256 bytes. u64 elements keep the buffer aligned.
    let mut buf = [0_u64; 64];
    let mut needed = 0_u32;
    // SAFETY: valid token handle; `buf` is writable for its size; `needed` is a valid pointer.
    let ok = unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            buf.as_mut_ptr().cast::<c_void>(),
            u32::try_from(size_of_val(&buf)).unwrap_or(0),
            &raw mut needed,
        )
    };
    last_error_if_false(ok, "GetTokenInformation")?;
    // SAFETY: on success the buffer starts with a TOKEN_USER; the buffer is 8-byte aligned.
    let sid = unsafe { (*buf.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let mut text: *mut u16 = ptr::null_mut();
    // SAFETY: `sid` points into `buf`, alive for the call; `text` is a valid out pointer.
    let ok = unsafe { ConvertSidToStringSidW(sid, &raw mut text) };
    last_error_if_false(ok, "ConvertSidToStringSidW")?;
    let text = LocalBox(text.cast());
    // SAFETY: ConvertSidToStringSidW returns a NUL-terminated string.
    let s = unsafe { from_wide_ptr(text.0.cast::<u16>()) }?;
    Ok(s.to_string_lossy().into_owned())
}

/// SHA-256 of a file's contents (`BCryptHash`; no hashing crate). Reads the whole file: meant for
/// executables of a few MB.
///
/// # Errors
/// I/O errors; the NTSTATUS of `BCryptHash`.
pub fn sha256_file(path: &Path) -> Result<[u8; 32]> {
    let data = std::fs::read(path).map_err(|e| io_error(&e, "ReadFile"))?;
    sha256(&data)
}

/// SHA-256 of `data`.
///
/// # Errors
/// The NTSTATUS of `BCryptHash`.
pub fn sha256(data: &[u8]) -> Result<[u8; 32]> {
    let mut out = [0_u8; 32];
    let len = u32::try_from(data.len()).map_err(|_| Error::MessageTooLarge(data.len()))?;
    // SAFETY: the pseudo-handle needs no open/close; `data` and `out` are valid for their
    // lengths; no secret (unkeyed hash).
    let status = unsafe {
        BCryptHash(
            BCRYPT_SHA256_ALG_HANDLE,
            ptr::null(),
            0,
            data.as_ptr(),
            len,
            out.as_mut_ptr(),
            32,
        )
    };
    if status == 0 {
        Ok(out)
    } else {
        Err(Error::Os(OsError {
            code: status,
            api: "BCryptHash",
        }))
    }
}

/// `\\.\pipe\<name>` for a plain name: ASCII letters, digits, `-`, `_` and `.`, at most 200.
fn pipe_path(name: &str) -> Result<WideCString> {
    let plain = !name.is_empty()
        && name.len() <= 200
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    if !plain {
        return Err(Error::InvalidName(name.to_owned()));
    }
    WideCString::new(format!(r"\\.\pipe\{name}"))
}

/// `S-1-` followed by dash-separated decimal numbers: nothing that could close an SDDL ACE.
fn is_sid(s: &str) -> bool {
    s.strip_prefix("S-1-").is_some_and(|rest| {
        !rest.is_empty()
            && rest
                .split('-')
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
    })
}

fn owned(handle: HANDLE, api: &'static str) -> Result<OwnedHandle> {
    if handle == INVALID_HANDLE_VALUE || handle.is_null() {
        return Err(last_os_error(api).into());
    }
    // SAFETY: a valid handle that nothing else owns.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

/// The `HRESULT` an [`OsError`] carries for a Win32 code.
fn hresult(code: u32) -> i32 {
    OsError::from_win32(code, "").code
}

#[cfg(test)]
// What Wine doesn't implement (first-instance squatting, integrity labels, ACL checks, remote
// refusal) is checked on real Windows by examples/pipe_t3.rs in the T3 job.
mod tests {
    use std::thread;

    use super::*;

    fn unique(tag: &str) -> String {
        format!("dpro-test-{tag}-{}", std::process::id())
    }

    #[test]
    fn round_trip_between_two_ends() {
        let name = unique("rt");
        let server = Pipe::create(&name, &current_user_sid().unwrap()).unwrap();
        let client_name = name.clone();
        let client = thread::spawn(move || {
            let c = Pipe::connect(&client_name, 2_000).unwrap();
            assert_eq!(c.peer_pid().unwrap(), std::process::id());
            c.send(b"hello").unwrap();
            let mut buf = Vec::new();
            c.recv(&mut buf).unwrap();
            buf
        });
        server.accept().unwrap();
        assert_eq!(server.peer_pid().unwrap(), std::process::id());
        let mut buf = Vec::new();
        server.recv(&mut buf).unwrap();
        assert_eq!(buf, b"hello");
        // Larger than one read chunk: arrives as one message.
        let big: Vec<u8> = (0..200_000_u32).map(|i| (i % 251) as u8).collect();
        server.send(&big).unwrap();
        assert_eq!(client.join().unwrap(), big);
    }

    #[test]
    fn two_fronts_on_two_instances() {
        let name = unique("two");
        let first = Pipe::create(&name, &current_user_sid().unwrap()).unwrap();
        let second = first.next_instance().unwrap();
        let fronts: Vec<_> = [b"one".as_slice(), b"two".as_slice()]
            .into_iter()
            .map(|text| {
                let name = name.clone();
                thread::spawn(move || {
                    let c = Pipe::connect(&name, 2_000).unwrap();
                    c.send(text).unwrap();
                    let mut buf = Vec::new();
                    c.recv(&mut buf).unwrap();
                    assert_eq!(buf, text);
                    assert!(matches!(c.next_instance(), Err(Error::ClientEnd)));
                })
            })
            .collect();
        let mut seen = Vec::new();
        for server in [&first, &second] {
            server.accept().unwrap();
            let mut buf = Vec::new();
            server.recv(&mut buf).unwrap();
            server.send(&buf).unwrap();
            seen.push(buf);
        }
        for front in fronts {
            front.join().unwrap();
        }
        seen.sort();
        assert_eq!(seen, [b"one".to_vec(), b"two".to_vec()]);
    }

    #[test]
    fn refuses_names_and_sids_that_could_inject() {
        let sid = current_user_sid().unwrap();
        for name in ["", r"a\b", "a b", "a/b", &"x".repeat(201)] {
            assert!(
                matches!(Pipe::create(name, &sid), Err(Error::InvalidName(_))),
                "{name:?}"
            );
        }
        for bad in ["", "S-1-", "S-1-5-21-1)(A;;GA;;;WD", "WD", "S-1-5--1"] {
            assert!(
                matches!(
                    Pipe::create(&unique("sid"), bad),
                    Err(Error::InvalidName(_))
                ),
                "{bad:?}"
            );
        }
        assert!(is_sid(&sid), "{sid}");
    }

    #[test]
    fn oversized_messages_are_refused_before_sending() {
        let name = unique("big");
        let server = Pipe::create(&name, &current_user_sid().unwrap()).unwrap();
        let too_big = vec![0_u8; MAX_MESSAGE + 1];
        assert!(matches!(
            server.send(&too_big),
            Err(Error::MessageTooLarge(_))
        ));
    }

    #[test]
    fn connecting_to_no_server_fails() {
        assert!(Pipe::connect(&unique("none"), 100).is_err());
    }

    #[test]
    fn sha256_matches_the_standard_vector() {
        let h = sha256(b"abc").unwrap();
        let want = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let want: Vec<u8> = (0..want.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&want[i..i + 2], 16).unwrap())
            .collect();
        assert_eq!(h.as_slice(), want.as_slice());
    }

    #[test]
    fn reads_its_own_user_and_session() {
        let sid = current_user_sid().unwrap();
        assert_eq!(process_user_sid(std::process::id()).unwrap(), sid);
        // Session 0 is services'; tests run in a user session, but Wine has only one.
        let _session = current_session_id().unwrap();
    }

    #[test]
    fn finds_its_own_image() {
        let image = process_image(std::process::id()).unwrap();
        let exe = std::env::current_exe().unwrap();
        assert_eq!(
            image.file_name().map(std::ffi::OsStr::to_ascii_lowercase),
            exe.file_name().map(std::ffi::OsStr::to_ascii_lowercase)
        );
    }
}
