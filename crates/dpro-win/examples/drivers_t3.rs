//! T3 probe of the DISM driver calls on real Windows (roadmap 2.5). It measures what the drivers
//! design needs: call times, what DISM reports for a staged package, and the errors it returns.
//! It also runs the round trip that undoing a removal relies on. A dev tool, never shipped; the T3
//! job runs it.
//!
//! ```text
//! drivers_t3 offline <image root>   lists the drivers, then stages, removes and re-adds a test
//!                                   package from a copy of its store folder, leaving the
//!                                   image as it was
//! drivers_t3 online                 lists the drivers and makes two calls that change nothing
//! ```
//!
//! It isn't a `#[test]`: the dpro-win test binary must not import `dismapi.dll`, or Wine (T2)
//! can't load it.

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
        ffi::{OsStr, OsString},
        fmt::{Debug, Display},
        fs,
        io::{self, Write as _},
        path::{Path, PathBuf},
        process::ExitCode,
        time::Instant,
    };

    use dpro_sys::dism::{DismDriverSignatureSigned, DismDriverSignatureUnsigned};
    use dpro_win::dism::{Completion, DismApi, DriverPackageInfo, LogLevel, Session};

    const USAGE: &str = "usage: drivers_t3 offline <image root> | drivers_t3 online";
    const INF_NAME: &str = "dpro-t3-null.inf";
    const HWID: &str = r"ROOT\DPRO_T3_NULL";
    /// One root-enumerated model that installs nothing (a null service) and copies no file, so
    /// staging it changes only the driver store. No catalog: the package is unsigned.
    const INF: &str = r#"; DISM Pro T3 test package: a device that never exists. Staged and removed by
; the drivers_t3 probe on a throwaway image; never installed.
[Version]
Signature   = "$WINDOWS NT$"
Class       = System
ClassGuid   = {4d36e97d-e325-11ce-bfc1-08002be10318}
Provider    = %Provider%
DriverVer   = 11/03/2026,1.0.0.0
PnpLockdown = 1

[Manufacturer]
%Provider% = Models, NTamd64, NTarm64

[Models.NTamd64]
%Device% = NullInstall, ROOT\DPRO_T3_NULL

[Models.NTarm64]
%Device% = NullInstall, ROOT\DPRO_T3_NULL

[NullInstall.NT]

[NullInstall.NT.Services]
AddService = , 0x00000002

[Strings]
Provider = "DISM Pro T3"
Device   = "DISM Pro T3 null device"
"#;

    type R<T> = Result<T, String>;

    trait Ctx<T> {
        fn ctx(self, what: &str) -> R<T>;
    }

    impl<T, E: Display> Ctx<T> for Result<T, E> {
        fn ctx(self, what: &str) -> R<T> {
            self.map_err(|e| format!("{what}: {e}"))
        }
    }

    macro_rules! say {
        ($($arg:tt)*) => {{
            // ignore: a closed stdout only loses the report.
            let _ = writeln!(io::stdout().lock(), $($arg)*);
        }};
    }

    pub(super) fn main() -> ExitCode {
        let args: Vec<OsString> = env::args_os().skip(1).collect();
        let result = match args.as_slice() {
            [mode, root] if mode == "offline" => offline(Path::new(root)),
            [mode] if mode == "online" => online(),
            _ => Err(USAGE.to_owned()),
        };
        match result {
            Ok(()) => {
                say!("PASS");
                ExitCode::SUCCESS
            }
            Err(e) => {
                // ignore: nothing else to report it on.
                let _ = writeln!(io::stderr().lock(), "FAIL: {e}");
                ExitCode::FAILURE
            }
        }
    }

    fn ms(since: Instant) -> f64 {
        since.elapsed().as_secs_f64() * 1e3
    }

    /// Runs `f` once and prints how long it took.
    fn timed<T>(label: &str, f: impl FnOnce() -> T) -> T {
        let t = Instant::now();
        let r = f();
        say!("{label}: {:.1} ms", ms(t));
        r
    }

    /// Runs `f` three times, prints every time and returns the last result.
    fn timed3<T>(label: &str, mut f: impl FnMut() -> T) -> T {
        let mut times = Vec::with_capacity(3);
        let mut last;
        loop {
            let t = Instant::now();
            last = f();
            times.push(format!("{:.1}", ms(t)));
            if times.len() == 3 {
                break;
            }
        }
        say!("{label}: {} ms", times.join(" / "));
        last
    }

    /// Prints whether a call that should change nothing was accepted or refused, and how.
    fn refusal<T: Debug, E: Display>(label: &str, r: Result<T, E>) {
        match r {
            Ok(v) => say!("{label}: accepted ({v:?})"),
            Err(e) => say!("{label}: refused: {e}"),
        }
    }

    /// An empty folder under the temp directory.
    fn scratch(name: &str) -> R<PathBuf> {
        let dir = env::temp_dir().join("dpro-t3-drivers").join(name);
        if dir.exists() {
            fs::remove_dir_all(&dir).ctx("clear the scratch folder")?;
        }
        fs::create_dir_all(&dir).ctx("create the scratch folder")?;
        Ok(dir)
    }

    /// Copies a folder tree; returns the files copied, relative to `from`.
    fn copy_dir(from: &Path, to: &Path) -> R<Vec<PathBuf>> {
        let mut copied = Vec::new();
        let mut stack = vec![PathBuf::new()];
        while let Some(rel) = stack.pop() {
            fs::create_dir_all(to.join(&rel)).ctx("create a backup folder")?;
            for entry in fs::read_dir(from.join(&rel)).ctx("read the store folder")? {
                let entry = entry.ctx("read the store folder")?;
                let rel = rel.join(entry.file_name());
                if entry.file_type().ctx("stat a store file")?.is_dir() {
                    stack.push(rel);
                } else {
                    fs::copy(entry.path(), to.join(&rel)).ctx("copy a store file")?;
                    copied.push(rel);
                }
            }
        }
        Ok(copied)
    }

    fn is_ours(p: &DriverPackageInfo) -> bool {
        Path::new(&p.original_file)
            .file_name()
            .is_some_and(|n| n.eq_ignore_ascii_case(INF_NAME))
    }

    fn staged(s: &Session<'_>) -> R<Option<DriverPackageInfo>> {
        Ok(s.drivers(false)
            .ctx("DismGetDrivers")?
            .into_iter()
            .find(is_ours))
    }

    /// Prints how a call completed, and reopens the session when DISM asks for it.
    fn settle(s: &mut Session<'_>, label: &str, c: Completion) -> R<()> {
        say!("{label}: {c:?}");
        if c == Completion::ReloadSession {
            s.reload().ctx("reopen the session")?;
        }
        Ok(())
    }

    fn summary(label: &str, packages: &[DriverPackageInfo]) {
        let count = |f: fn(&DriverPackageInfo) -> bool| packages.iter().filter(|p| f(p)).count();
        say!(
            "{label}: {} packages, {} inbox, {} boot-critical, {} signed, {} unsigned",
            packages.len(),
            count(|p| p.inbox),
            count(|p| p.boot_critical),
            count(|p| p.signature == DismDriverSignatureSigned),
            count(|p| p.signature == DismDriverSignatureUnsigned),
        );
    }

    /// Times both listings and prints their make-up, then one sample package (the first one
    /// `pick` accepts) with its `DismGetDriverInfo`. Returns the third-party list.
    fn listings(
        s: &Session<'_>,
        pick: fn(&DriverPackageInfo) -> bool,
    ) -> R<Vec<DriverPackageInfo>> {
        let third = timed3("DismGetDrivers, third-party", || s.drivers(false))
            .ctx("DismGetDrivers, third-party")?;
        let all = timed3("DismGetDrivers, all", || s.drivers(true)).ctx("DismGetDrivers, all")?;
        summary("third-party", &third);
        summary("all", &all);
        if all.is_empty() {
            return Err("no driver packages at all".to_owned());
        }
        if let Some(p) = all.iter().find(|p| pick(p)) {
            say!("sample package: {p:?}");
            let (devices, package) = timed("DismGetDriverInfo, by published name", || {
                s.driver_info(&p.published_name)
            })
            .ctx("DismGetDriverInfo, by published name")?;
            say!(
                "  {} device(s), package reported: {}",
                devices.len(),
                package.is_some()
            );
        }
        Ok(third)
    }

    /// Writes the test INF and checks what `DismGetDriverInfo` reads from it.
    fn test_inf(s: &Session<'_>) -> R<PathBuf> {
        let inf = scratch("source")?.join(INF_NAME);
        fs::write(&inf, INF).ctx("write the test INF")?;
        let (devices, package) = timed("DismGetDriverInfo, INF file", || {
            s.driver_info(inf.as_os_str())
        })
        .ctx("DismGetDriverInfo, INF file")?;
        say!("INF devices: {devices:?}");
        say!("INF package: {package:?}");
        if !devices
            .iter()
            .any(|d| d.hardware_id.eq_ignore_ascii_case(HWID))
        {
            return Err(format!("{HWID} isn't among the INF's devices"));
        }
        Ok(inf)
    }

    /// What DISM does with an unsigned package without `ForceUnsigned`, for the CLI's flag.
    /// Leaves the store as it was.
    fn unsigned_without_force(s: &mut Session<'_>, inf: &Path) -> R<()> {
        match s.add_driver(inf, false) {
            Ok(c) => {
                settle(s, "unsigned, without ForceUnsigned: accepted", c)?;
                if let Some(p) = staged(s)? {
                    let c = s.remove_driver(&p.published_name).ctx("DismRemoveDriver")?;
                    settle(s, "  removed again", c)?;
                }
            }
            Err(e) => say!("unsigned, without ForceUnsigned: refused: {e}"),
        }
        Ok(())
    }

    fn offline(root: &Path) -> R<()> {
        let api = DismApi::initialize(LogLevel::Errors, None, None).ctx("DismInitialize")?;
        let mut s = timed("open the offline session", || {
            Session::open_offline(&api, root, None)
        })
        .ctx("DismOpenSession")?;
        let before = listings(&s, |p| p.inbox)?;
        if before.iter().any(is_ours) {
            return Err(format!(
                "{INF_NAME} is already staged: a previous run didn't clean up"
            ));
        }
        let inf = test_inf(&s)?;
        unsigned_without_force(&mut s, &inf)?;

        let c = timed("DismAddDriver", || s.add_driver(&inf, true)).ctx("DismAddDriver")?;
        settle(&mut s, "  completion", c)?;
        let added = staged(&s)?.ok_or("the package isn't listed after DismAddDriver")?;
        say!("staged: {added:?}");
        if added.inbox {
            return Err("the test package is reported as inbox".to_owned());
        }

        // Undoing a removal re-adds a copy of the package's store folder: check that
        // OriginalFileName points into it.
        let store_inf = PathBuf::from(&added.original_file);
        let store_dir = store_inf
            .parent()
            .ok_or("OriginalFileName has no folder")?
            .to_owned();
        say!(
            "store folder: {} (in the image's FileRepository: {})",
            store_dir.display(),
            store_dir.starts_with(root.join(r"Windows\System32\DriverStore\FileRepository"))
        );
        let backup = scratch("backup")?;
        let files = copy_dir(&store_dir, &backup)?;
        say!("copied {} file(s): {files:?}", files.len());

        let c = timed("DismRemoveDriver", || {
            s.remove_driver(&added.published_name)
        })
        .ctx("DismRemoveDriver")?;
        settle(&mut s, "  completion", c)?;
        if staged(&s)?.is_some() {
            return Err("still listed after DismRemoveDriver".to_owned());
        }
        say!(
            "store folder still there after removal: {}",
            store_dir.exists()
        );

        let copy_inf = backup.join(
            store_inf
                .file_name()
                .ok_or("OriginalFileName has no file name")?,
        );
        let c = timed("DismAddDriver, from the store copy", || {
            s.add_driver(&copy_inf, true)
        })
        .ctx("DismAddDriver, from the store copy")?;
        settle(&mut s, "  completion", c)?;
        let again = staged(&s)?.ok_or("not listed after re-adding the store copy")?;
        say!("re-added as {:?}", again.published_name);
        if (
            again.version,
            again.date,
            &again.class_guid,
            &again.provider,
        ) != (
            added.version,
            added.date,
            &added.class_guid,
            &added.provider,
        ) {
            return Err(format!("the re-added package differs: {again:?}"));
        }

        refusal(
            "removing an unknown published name",
            s.remove_driver(OsStr::new("oem99999.inf")),
        );
        let c = s
            .remove_driver(&again.published_name)
            .ctx("DismRemoveDriver, cleanup")?;
        settle(&mut s, "cleanup: removed", c)?;
        if staged(&s)?.is_some() {
            return Err("cleanup failed: the test package is still staged".to_owned());
        }
        Ok(())
    }

    fn online() -> R<()> {
        let api = DismApi::initialize(LogLevel::Errors, None, None).ctx("DismInitialize")?;
        let s = timed("open the online session", || Session::open_online(&api))
            .ctx("DismOpenSession")?;
        listings(&s, |p| !p.inbox)?;
        // Whether DISM changes drivers online at all (its reference says offline images): two
        // calls that change nothing either way.
        refusal(
            "online: removing an unknown published name",
            s.remove_driver(OsStr::new("oem99999.inf")),
        );
        refusal(
            "online: adding an INF that doesn't exist",
            s.add_driver(Path::new(r"C:\dpro-t3-missing\missing.inf"), false),
        );
        Ok(())
    }
}
