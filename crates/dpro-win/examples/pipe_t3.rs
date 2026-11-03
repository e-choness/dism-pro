//! T3 probe of the broker pipe on real Windows (roadmap 2.7; TD-0013 §7, §15). It times round
//! trips against the budget and checks what Wine can't: the pipe's label lets a medium-integrity
//! client in and keeps a low one out, remote clients are refused, and the pipe can't be created
//! under a name a squatter already holds. A dev tool, never shipped; the T3 job runs it.
//!
//! ```text
//! pipe_t3                          runs every check, one line each; exit = number of failures
//! pipe_t3 echo <name> <server pid> (child) connects, checks the server's pid, echoes until closed
//! pipe_t3 open <name>              (child) opens the pipe once; exit 0 or the Win32 error code
//! ```

use std::process::ExitCode;

fn main() -> ExitCode {
    #[cfg(windows)]
    {
        probe::main()
    }
    #[cfg(not(windows))]
    {
        ExitCode::FAILURE
    }
}

#[cfg(windows)]
mod probe {
    use std::{
        env,
        ffi::{OsStr, c_void},
        fmt::Display,
        fs::OpenOptions,
        io::{self, Write as _},
        os::windows::{
            ffi::OsStrExt,
            io::{AsRawHandle, FromRawHandle, OwnedHandle},
        },
        path::Path,
        process::{Command, ExitCode},
        ptr,
        time::Instant,
    };

    use dpro_win::{
        error::Error,
        pipe::{self, Pipe},
    };
    use windows_sys::Win32::{
        Foundation::{HANDLE, INVALID_HANDLE_VALUE, LocalFree, WAIT_OBJECT_0},
        Security::{
            Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, ConvertStringSidToSidW,
                SDDL_REVISION_1,
            },
            CreateRestrictedToken, DISABLE_MAX_PRIVILEGE, GetLengthSid, GetSidSubAuthority,
            GetSidSubAuthorityCount, GetTokenInformation, PSECURITY_DESCRIPTOR, PSID,
            SECURITY_ATTRIBUTES, SID_AND_ATTRIBUTES, SetTokenInformation, TOKEN_ADJUST_DEFAULT,
            TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
            TokenIntegrityLevel,
        },
        Storage::FileSystem::PIPE_ACCESS_DUPLEX,
        System::{
            Pipes::{
                CreateNamedPipeW, PIPE_READMODE_MESSAGE, PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_TYPE_MESSAGE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
            },
            Threading::{
                CreateProcessAsUserW, GetCurrentProcess, GetExitCodeProcess, OpenProcessToken,
                PROCESS_INFORMATION, STARTUPINFOW, TerminateProcess, WaitForSingleObject,
            },
        },
    };

    type R<T> = Result<T, String>;

    const USAGE: &str = "usage: pipe_t3 | pipe_t3 echo <name> <server pid> | pipe_t3 open <name> \
                         | pipe_t3 instance <name>";
    const ADMINISTRATORS: &str = "S-1-5-32-544";
    const MEDIUM: &str = "S-1-16-8192";
    const LOW: &str = "S-1-16-4096";
    const MEDIUM_RID: u32 = 0x2000;
    /// winnt.h; `SystemServices` isn't a feature this workspace enables.
    const SE_GROUP_INTEGRITY: u32 = 0x20;
    const ACCESS_DENIED: u32 = 5;
    /// A typical request, the largest tool result (TD-0013 §5.6) and the budget's size.
    const SIZES: [usize; 3] = [1024, 32 * 1024, 64 * 1024];
    const WARMUP: usize = 200;
    const ROUNDS: usize = 5_000;
    /// TD-0013 §7: one request and its response, up to 64 KiB, p99.
    const BUDGET_NS: u64 = 1_000_000;

    macro_rules! say {
        ($($arg:tt)*) => {{
            // ignore: a closed stdout only loses the report.
            let _ = writeln!(io::stdout().lock(), $($arg)*);
        }};
    }

    pub(super) fn main() -> ExitCode {
        let args: Vec<String> = env::args_os()
            .skip(1)
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        match args.as_slice() {
            [] => run_all(),
            ["echo", name, pid] => echo(name, pid),
            ["open", name] => open(name),
            ["instance", name] => instance(name),
            _ => {
                say!("{USAGE}");
                ExitCode::from(2)
            }
        }
    }

    #[derive(Default)]
    struct Report {
        failed: u8,
    }

    impl Report {
        fn check(&mut self, name: &str, result: R<String>) {
            match result {
                Ok(detail) => say!("PASS {name}: {detail}"),
                Err(e) => {
                    self.failed = self.failed.saturating_add(1);
                    say!("FAIL {name}: {e}");
                }
            }
        }
    }

    fn run_all() -> ExitCode {
        let mut report = Report::default();
        let (sid, level) = match pipe::current_user_sid()
            .map_err(err)
            .and_then(|sid| Ok((sid, integrity()?)))
        {
            Ok(v) => v,
            Err(e) => {
                say!("FAIL identity: {e}");
                return ExitCode::FAILURE;
            }
        };
        say!("INFO identity: {} integrity", level_name(level));
        if let Err(e) = peer_and_round_trips(&sid, &mut report) {
            report.check("peer and round trips", Err(e));
        }
        report.check("squatter's name refused", squatter(&sid));
        if level > MEDIUM_RID {
            report.check(
                "medium-integrity client allowed",
                labelled_child(&sid, MEDIUM, "open").and_then(|code| match code {
                    0 => Ok("opened the elevated server's pipe".to_owned()),
                    code => Err(format!("refused with {code}")),
                }),
            );
            say!("INFO label needed: {}", unlabelled_control(&sid));
            report.check(
                "medium-integrity process can't add an instance",
                labelled_child(&sid, MEDIUM, "instance").and_then(|code| match code {
                    ACCESS_DENIED => Ok("access denied".to_owned()),
                    0 => Err("created an instance of the broker's pipe".to_owned()),
                    code => Err(format!("failed with {code:#x}, not access denied")),
                }),
            );
        } else {
            say!("INFO medium-integrity client: skipped, this process isn't above medium");
        }
        report.check(
            "low-integrity client refused",
            labelled_child(&sid, LOW, "open").and_then(|code| match code {
                ACCESS_DENIED => Ok("access denied".to_owned()),
                0 => Err("opened the pipe".to_owned()),
                code => Err(format!("failed with {code:#x}, not access denied")),
            }),
        );
        report.check("remote client refused", remote(&sid));
        if report.failed == 0 {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(report.failed)
        }
    }

    /// Server and an echoing child: each end checks the other's pid, the server checks the
    /// child's image, then times round trips at each size.
    fn peer_and_round_trips(sid: &str, report: &mut Report) -> R<()> {
        let name = unique("rt");
        let server = Pipe::create(&name, sid).map_err(err)?;
        let exe = env::current_exe().map_err(err)?;
        let mut child = Command::new(&exe)
            .args(["echo", &name, &std::process::id().to_string()])
            .spawn()
            .map_err(err)?;
        server.accept().map_err(err)?;
        report.check("peer identity", peer_identity(&server, child.id(), &exe));
        for size in SIZES {
            report.check(
                &format!("round trip {} KiB", size / 1024),
                round_trip(&server, size),
            );
        }
        drop(server);
        let status = child.wait().map_err(err)?;
        report.check(
            "client saw the server's pid",
            if status.success() {
                Ok("echo client exited cleanly".to_owned())
            } else {
                Err(format!("echo client exited with {status}"))
            },
        );
        Ok(())
    }

    fn peer_identity(server: &Pipe, child: u32, exe: &Path) -> R<String> {
        let peer = server.peer_pid().map_err(err)?;
        if peer != child {
            return Err(format!("client pid {peer}, spawned {child}"));
        }
        let image = pipe::process_image(peer).map_err(err)?;
        if !image
            .to_string_lossy()
            .eq_ignore_ascii_case(&exe.to_string_lossy())
        {
            return Err(format!(
                "client image {} isn't {}",
                image.display(),
                exe.display()
            ));
        }
        let t = Instant::now();
        let theirs = pipe::sha256_file(&image).map_err(err)?;
        let hashed = t.elapsed();
        let size = std::fs::metadata(&image).map_err(err)?.len();
        if theirs != pipe::sha256_file(exe).map_err(err)? {
            return Err("image hashes differ".to_owned());
        }
        Ok(format!(
            "pid {peer}, image matches; SHA-256 of {} KiB in {}",
            size / 1024,
            us(u64::try_from(hashed.as_nanos()).unwrap_or(u64::MAX))
        ))
    }

    fn round_trip(server: &Pipe, size: usize) -> R<String> {
        let message: Vec<u8> = (0..size)
            .map(|i| u8::try_from(i % 251).unwrap_or(0))
            .collect();
        let mut buf = Vec::with_capacity(size);
        let mut times = Vec::with_capacity(ROUNDS);
        for round in 0..WARMUP + ROUNDS {
            let t = Instant::now();
            server.send(&message).map_err(err)?;
            server.recv(&mut buf).map_err(err)?;
            let ns = u64::try_from(t.elapsed().as_nanos()).unwrap_or(u64::MAX);
            if buf != message {
                return Err(format!("echo differs in round {round}"));
            }
            if round >= WARMUP {
                times.push(ns);
            }
        }
        times.sort_unstable();
        let at = |q: usize| times.get(ROUNDS * q / 100).copied().unwrap_or(u64::MAX);
        let p99 = at(99);
        let verdict = if p99 <= BUDGET_NS {
            "within"
        } else {
            "OVER (advisory on a shared runner)"
        };
        Ok(format!(
            "p50 {} / p99 {} / max {} over {ROUNDS}; {verdict} the 1 ms p99 budget",
            us(at(50)),
            us(p99),
            us(times.last().copied().unwrap_or(u64::MAX)),
        ))
    }

    /// Someone else already serves the name, with room for more instances: our first-instance
    /// create must fail rather than join them.
    fn squatter(sid: &str) -> R<String> {
        let name = unique("squat");
        let _theirs = raw_pipe(&name, None, false).map_err(err)?;
        match Pipe::create(&name, sid) {
            Ok(_) => Err("created an instance under the squatter's name".to_owned()),
            Err(e) => Ok(format!("refused: {e}")),
        }
    }

    /// A child at `level` runs `mode` (`open` or `instance`) on the production pipe; its exit
    /// code.
    fn labelled_child(sid: &str, level: &str, mode: &str) -> R<u32> {
        let name = unique(&format!("label-{mode}"));
        let _server = Pipe::create(&name, sid).map_err(err)?;
        spawn_at(level, &[mode, &name])
    }

    /// The same user ACE without the label, opened at medium: shows whether the label is what
    /// lets the unelevated front in.
    fn unlabelled_control(sid: &str) -> String {
        let name = unique("nolabel");
        let ace = format!("D:P(A;;{:#x};;;{sid})", pipe::CLIENT_ACCESS);
        let result = raw_pipe(&name, Some(&ace), true)
            .map_err(err)
            .and_then(|_server| spawn_at(MEDIUM, &["open", &name]));
        match result {
            Ok(ACCESS_DENIED) => "without it a medium client is denied (label required)".to_owned(),
            Ok(0) => "a medium client got in without it (label not required here)".to_owned(),
            Ok(code) => format!("control failed with {code:#x}"),
            Err(e) => format!("control not run: {e}"),
        }
    }

    /// Opens the pipe through the SMB loopback, which the pipe sees as a remote client. A pipe
    /// without the reject flag is the control.
    fn remote(sid: &str) -> R<String> {
        let ours = unique("remote");
        let _server = Pipe::create(&ours, sid).map_err(err)?;
        let control = unique("remote-control");
        let _control = raw_pipe(
            &control,
            Some(&format!("D:P(A;;GRGW;;;{sid})S:(ML;;NW;;;ME)")),
            false,
        )
        .map_err(err)?;
        let via = |name: &str| {
            OpenOptions::new()
                .read(true)
                .write(true)
                .open(format!(r"\\localhost\pipe\{name}"))
        };
        let control = match via(&control) {
            Ok(_) => "the same open of a pipe without the flag succeeded".to_owned(),
            Err(e) => format!("inconclusive: the control without the flag failed too ({e})"),
        };
        match via(&ours) {
            Ok(_) => Err(format!("opened through \\\\localhost; {control}")),
            Err(e) => Ok(format!("{e}; {control}")),
        }
    }

    /// Child: connect, check the server is `server_pid`, echo every message until the server
    /// closes. Exit 2: couldn't connect; 3: wrong server.
    fn echo(name: &str, server_pid: &str) -> ExitCode {
        let Ok(pipe) = Pipe::connect(name, 5_000) else {
            return ExitCode::from(2);
        };
        if pipe.peer_pid().ok().map(|p| p.to_string()).as_deref() != Some(server_pid) {
            return ExitCode::from(3);
        }
        let mut buf = Vec::with_capacity(64 * 1024);
        while pipe.recv(&mut buf).is_ok() {
            if pipe.send(&buf).is_err() {
                break;
            }
        }
        ExitCode::SUCCESS
    }

    /// Child: open once. Exit 0, or the Win32 error code (255 if it doesn't fit).
    fn open(name: &str) -> ExitCode {
        match Pipe::connect(name, 2_000) {
            Ok(_) => ExitCode::SUCCESS,
            Err(Error::Os(e)) => {
                ExitCode::from(u8::try_from(e.hresult_bits() & 0xFFFF).unwrap_or(u8::MAX))
            }
            Err(_) => ExitCode::from(u8::MAX),
        }
    }

    /// Child: try to add an instance of someone else's pipe. Exit 0, or the Win32 error code.
    fn instance(name: &str) -> ExitCode {
        match raw_pipe(name, None, true) {
            Ok(_) => ExitCode::SUCCESS,
            Err(e) => ExitCode::from(
                e.raw_os_error()
                    .and_then(|c| u8::try_from(c).ok())
                    .unwrap_or(u8::MAX),
            ),
        }
    }

    /// This process's mandatory integrity RID (0x2000 medium, 0x3000 high).
    fn integrity() -> R<u32> {
        let token = own_token(TOKEN_QUERY)?;
        // TOKEN_MANDATORY_LABEL plus its SID; u64 elements keep it aligned.
        let mut buf = [0_u64; 16];
        let mut needed = 0_u32;
        // SAFETY: valid token handle; `buf` is writable for its size; `needed` is valid.
        let ok = unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                TokenIntegrityLevel,
                buf.as_mut_ptr().cast::<c_void>(),
                u32::try_from(size_of_val(&buf)).unwrap_or(0),
                &raw mut needed,
            )
        };
        if ok == 0 {
            return Err(last("GetTokenInformation"));
        }
        // SAFETY: on success `buf` starts with an aligned TOKEN_MANDATORY_LABEL whose SID
        // points into `buf`.
        let sid = unsafe { (*buf.as_ptr().cast::<TOKEN_MANDATORY_LABEL>()).Label.Sid };
        // SAFETY: `sid` is a valid SID; the count pointer is into it.
        let count = unsafe { *GetSidSubAuthorityCount(sid) };
        let last_index = u32::from(count)
            .checked_sub(1)
            .ok_or("integrity SID has no RID")?;
        // SAFETY: `last_index` is below the SID's sub-authority count.
        Ok(unsafe { *GetSidSubAuthority(sid, last_index) })
    }

    /// Runs this exe with `args` under this process's token filtered the way UAC filters an
    /// administrator's standard session (Administrators deny-only, privileges removed) and
    /// labelled `level` (an integrity SID); returns its exit code.
    fn spawn_at(level: &str, args: &[&str]) -> R<u32> {
        let token =
            own_token(TOKEN_DUPLICATE | TOKEN_QUERY | TOKEN_ADJUST_DEFAULT | TOKEN_ASSIGN_PRIMARY)?;
        let admins = string_sid(ADMINISTRATORS)?;
        let deny_only = SID_AND_ATTRIBUTES {
            Sid: admins.0,
            Attributes: 0,
        };
        let mut primary: HANDLE = ptr::null_mut();
        // SAFETY: valid primary token handle; one SID to disable, which outlives the call; no
        // privileges listed (DISABLE_MAX_PRIVILEGE removes them) or restricting SIDs; `primary`
        // is a valid out pointer.
        let ok = unsafe {
            CreateRestrictedToken(
                token.as_raw_handle(),
                DISABLE_MAX_PRIVILEGE,
                1,
                &raw const deny_only,
                0,
                ptr::null(),
                0,
                ptr::null(),
                &raw mut primary,
            )
        };
        if ok == 0 {
            return Err(last("CreateRestrictedToken"));
        }
        // SAFETY: CreateRestrictedToken returned a handle nothing else owns.
        let primary = unsafe { OwnedHandle::from_raw_handle(primary) };
        let sid = string_sid(level)?;
        let label = TOKEN_MANDATORY_LABEL {
            Label: SID_AND_ATTRIBUTES {
                Sid: sid.0,
                Attributes: SE_GROUP_INTEGRITY,
            },
        };
        // SAFETY: `sid.0` is a valid SID.
        let sid_len = unsafe { GetLengthSid(sid.0) };
        let len = u32::try_from(size_of::<TOKEN_MANDATORY_LABEL>()).unwrap_or(0) + sid_len;
        // SAFETY: the new token has the source's access, TOKEN_ADJUST_DEFAULT included; `label`
        // and its SID outlive the call.
        let ok = unsafe {
            SetTokenInformation(
                primary.as_raw_handle(),
                TokenIntegrityLevel,
                (&raw const label).cast::<c_void>(),
                len,
            )
        };
        if ok == 0 {
            return Err(last("SetTokenInformation"));
        }
        run_as(&primary, args)
    }

    fn run_as(token: &OwnedHandle, args: &[&str]) -> R<u32> {
        let exe = env::current_exe().map_err(err)?;
        let app = wide(exe.as_os_str());
        // The arguments are plain pipe names; only the exe path needs quotes.
        let mut command = wide(format!("\"{}\" {}", exe.display(), args.join(" ")));
        let startup = STARTUPINFOW {
            cb: u32::try_from(size_of::<STARTUPINFOW>()).unwrap_or(0),
            ..STARTUPINFOW::default()
        };
        let mut info = PROCESS_INFORMATION::default();
        // SAFETY: valid primary token; `app` is NUL-terminated; `command` is a mutable
        // NUL-terminated buffer; `startup` and `info` are valid for the call.
        let ok = unsafe {
            CreateProcessAsUserW(
                token.as_raw_handle(),
                app.as_ptr(),
                command.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                0,
                0,
                ptr::null(),
                ptr::null(),
                &raw const startup,
                &raw mut info,
            )
        };
        if ok == 0 {
            return Err(last("CreateProcessAsUserW"));
        }
        // SAFETY: CreateProcessAsUserW returned two handles that nothing else owns.
        let (process, _thread) = unsafe {
            (
                OwnedHandle::from_raw_handle(info.hProcess),
                OwnedHandle::from_raw_handle(info.hThread),
            )
        };
        // SAFETY: valid process handle.
        if unsafe { WaitForSingleObject(process.as_raw_handle(), 30_000) } != WAIT_OBJECT_0 {
            // SAFETY: valid process handle. ignore: it is reported as a failure either way.
            let _ = unsafe { TerminateProcess(process.as_raw_handle(), 1) };
            return Err("the child didn't finish in 30 s".to_owned());
        }
        let mut code = 0_u32;
        // SAFETY: valid process handle; `code` is a valid out pointer.
        if unsafe { GetExitCodeProcess(process.as_raw_handle(), &raw mut code) } == 0 {
            return Err(last("GetExitCodeProcess"));
        }
        Ok(code)
    }

    fn string_sid(text: &str) -> R<Local> {
        let text = wide(text);
        let mut sid: PSID = ptr::null_mut();
        // SAFETY: `text` is NUL-terminated; `sid` is a valid out pointer.
        if unsafe { ConvertStringSidToSidW(text.as_ptr(), &raw mut sid) } == 0 {
            return Err(last("ConvertStringSidToSidW"));
        }
        Ok(Local(sid))
    }

    fn own_token(access: u32) -> R<OwnedHandle> {
        let mut token: HANDLE = ptr::null_mut();
        // SAFETY: GetCurrentProcess returns a pseudo handle; `token` is a valid out pointer.
        if unsafe { OpenProcessToken(GetCurrentProcess(), access, &raw mut token) } == 0 {
            return Err(last("OpenProcessToken"));
        }
        // SAFETY: OpenProcessToken returned a handle nothing else owns.
        Ok(unsafe { OwnedHandle::from_raw_handle(token) })
    }

    /// A message-mode pipe with any security: `None` is the default descriptor.
    fn raw_pipe(name: &str, sddl: Option<&str>, reject_remote: bool) -> io::Result<OwnedHandle> {
        let path = wide(format!(r"\\.\pipe\{name}"));
        let mut sd: PSECURITY_DESCRIPTOR = ptr::null_mut();
        if let Some(sddl) = sddl {
            let sddl = wide(sddl);
            // SAFETY: `sddl` is NUL-terminated; `sd` is a valid out pointer.
            let ok = unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    SDDL_REVISION_1,
                    &raw mut sd,
                    ptr::null_mut(),
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
        }
        let sd = Local(sd);
        let attributes = SECURITY_ATTRIBUTES {
            nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(0),
            lpSecurityDescriptor: sd.0,
            bInheritHandle: 0,
        };
        let remote = if reject_remote {
            PIPE_REJECT_REMOTE_CLIENTS
        } else {
            0
        };
        // SAFETY: `path` is NUL-terminated; `attributes` and its descriptor outlive the call.
        let handle = unsafe {
            CreateNamedPipeW(
                path.as_ptr(),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | remote,
                PIPE_UNLIMITED_INSTANCES,
                4096,
                4096,
                0,
                if sd.0.is_null() {
                    ptr::null()
                } else {
                    &raw const attributes
                },
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: CreateNamedPipeW returned a handle nothing else owns.
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    }

    /// A `LocalAlloc`ed buffer from a security API.
    struct Local(*mut c_void);

    impl Drop for Local {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: allocated by the API with LocalAlloc, freed once.
                // ignore: LocalFree only fails for an invalid pointer.
                let _ = unsafe { LocalFree(self.0) };
            }
        }
    }

    fn unique(tag: &str) -> String {
        format!("dpro-t3-{tag}-{}", std::process::id())
    }

    fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
        s.as_ref().encode_wide().chain([0]).collect()
    }

    fn level_name(rid: u32) -> String {
        match rid {
            0x1000 => "low".to_owned(),
            0x2000 => "medium".to_owned(),
            0x2100 => "medium-plus".to_owned(),
            0x3000 => "high".to_owned(),
            0x4000 => "system".to_owned(),
            rid => format!("{rid:#x}"),
        }
    }

    fn us(ns: u64) -> String {
        format!("{}.{} µs", ns / 1000, ns % 1000 / 100)
    }

    fn err(e: impl Display) -> String {
        e.to_string()
    }

    fn last(api: &str) -> String {
        format!("{api}: {}", io::Error::last_os_error())
    }
}
