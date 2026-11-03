//! T3 probe of the WIMGAPI wrapper, `dpro_win::wim`, on real Windows (roadmap 2.3). Wine has no
//! wimgapi.dll, so this is an example the T3 job runs rather than a unit test. `dism.exe` builds
//! the fixtures from a small directory: a one-image WIM, a two-image WIM and an ESD. The probe
//! checks that `WimFile` opens each and counts its images, also through a path longer than
//! `MAX_PATH`, and that a missing file comes back as the OS error. Capturing needs an elevated
//! session, which T3 runners have. A dev tool, never shipped.
//!
//! ```text
//! wim_t3     runs every check, one line each; exit = number of failures
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
        env, fs,
        io::{self, Write as _},
        os::windows::ffi::OsStrExt as _,
        path::{Path, PathBuf},
        process::{self, Command, ExitCode, Stdio},
        time::Instant,
    };

    use dpro_win::{error::Error, privilege, wim::WimFile};

    type R<T> = Result<T, String>;

    /// `HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND)`.
    const FILE_NOT_FOUND: u32 = 0x8007_0002;
    /// Long enough that only a `\\?\` path reaches the file.
    const LONG_PATH: usize = 300;

    macro_rules! say {
        ($($arg:tt)*) => {{
            // ignore: a closed stdout only loses the report.
            let _ = writeln!(io::stdout().lock(), $($arg)*);
        }};
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

    pub(super) fn main() -> ExitCode {
        let mut report = Report::default();
        match privilege::is_elevated() {
            Ok(true) => {}
            Ok(false) => report.check(
                "elevated",
                Err("DISM capture needs an elevated session".into()),
            ),
            Err(e) => report.check("elevated", Err(e.to_string())),
        }
        if report.failed == 0 {
            let root = env::temp_dir().join(format!("dpro-wim-t3-{}", process::id()));
            run(&mut report, &root);
            // ignore: a leftover temp folder on a throwaway runner costs nothing.
            let _ = fs::remove_dir_all(&root);
        }
        ExitCode::from(report.failed)
    }

    fn run(report: &mut Report, root: &Path) {
        let one = root.join("one.wim");
        let captured = capture(root, &one);
        let ok = captured.is_ok();
        report.check("capture with dism.exe", captured);
        if !ok {
            return;
        }
        report.check("one image", count(&one, 1));
        report.check(
            "two images",
            append(root, &one).and_then(|two| count(&two, 2)),
        );
        report.check("ESD", export_esd(root, &one).and_then(|esd| count(&esd, 1)));
        report.check("path longer than MAX_PATH", long_path(root, &one));
        report.check("missing file", missing(root));
    }

    /// A small tree (a text file, 1 MiB of data two folders down, an empty file) captured as
    /// one image.
    fn capture(root: &Path, wim: &Path) -> R<String> {
        let tree = root.join("tree");
        let deeper = tree.join("nested").join("deeper");
        fs::create_dir_all(&deeper).map_err(|e| format!("create {}: {e}", deeper.display()))?;
        let data: Vec<u8> = (0..=u8::MAX).cycle().take(1 << 20).collect();
        let files = [
            (tree.join("readme.txt"), b"DISM Pro WIM probe\r\n".to_vec()),
            (deeper.join("data.bin"), data),
            (tree.join("nested").join("empty.txt"), Vec::new()),
        ];
        for (path, bytes) in &files {
            fs::write(path, bytes).map_err(|e| format!("write {}: {e}", path.display()))?;
        }
        dism(&[
            "/Capture-Image".into(),
            path_arg("ImageFile", wim),
            path_arg("CaptureDir", &tree),
            "/Name:first".into(),
            "/Compress:fast".into(),
        ])?;
        Ok(format!("{} ({} files)", sized(wim), files.len()))
    }

    /// A copy of `one` with the same tree appended as a second image.
    fn append(root: &Path, one: &Path) -> R<PathBuf> {
        let two = root.join("two.wim");
        fs::copy(one, &two).map_err(|e| format!("copy to {}: {e}", two.display()))?;
        dism(&[
            "/Append-Image".into(),
            path_arg("ImageFile", &two),
            path_arg("CaptureDir", &root.join("tree")),
            "/Name:second".into(),
        ])?;
        Ok(two)
    }

    /// Image 1 of `one`, exported with recovery compression (an ESD).
    fn export_esd(root: &Path, one: &Path) -> R<PathBuf> {
        let esd = root.join("one.esd");
        dism(&[
            "/Export-Image".into(),
            path_arg("SourceImageFile", one),
            "/SourceIndex:1".into(),
            path_arg("DestinationImageFile", &esd),
            "/Compress:recovery".into(),
        ])?;
        Ok(esd)
    }

    /// `one` copied below a folder chain that takes its path past `MAX_PATH`.
    fn long_path(root: &Path, one: &Path) -> R<String> {
        let mut dir = root.join("long");
        while dir.as_os_str().encode_wide().count() < LONG_PATH {
            dir.push("a-folder-to-pass-max-path");
        }
        fs::create_dir_all(&dir).map_err(|e| format!("create the long folder: {e}"))?;
        let wim = dir.join("one.wim");
        fs::copy(one, &wim).map_err(|e| format!("copy to the long folder: {e}"))?;
        let units = wim.as_os_str().encode_wide().count();
        count(&wim, 1).map(|detail| format!("{units} UTF-16 units: {detail}"))
    }

    fn missing(root: &Path) -> R<String> {
        match WimFile::open_read(&root.join("missing.wim")) {
            Ok(_) => Err("opened a file that doesn't exist".into()),
            Err(Error::Os(e)) if e.hresult_bits() == FILE_NOT_FOUND => Ok(e.to_string()),
            Err(e) => Err(format!("want {FILE_NOT_FOUND:#010x}, got {e}")),
        }
    }

    /// Opens `path` with `WimFile` and checks it holds `want` images.
    fn count(path: &Path, want: u32) -> R<String> {
        let start = Instant::now();
        let wim = WimFile::open_read(path).map_err(|e| format!("open {}: {e}", path.display()))?;
        let got = wim.image_count();
        drop(wim);
        let ms = start.elapsed().as_secs_f64() * 1e3;
        if got == want {
            Ok(format!(
                "{got} image(s) in {}, open + count + close {ms:.1} ms",
                sized(path)
            ))
        } else {
            Err(format!("{got} image(s) in {}, want {want}", sized(path)))
        }
    }

    /// Runs `dism.exe` from System32 with `args` and `/English`.
    fn dism(args: &[String]) -> R<()> {
        let root = env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
        let exe = Path::new(&root).join("System32").join("dism.exe");
        let out = Command::new(&exe)
            .args(args)
            .arg("/English")
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("{}: {e}", exe.display()))?;
        if out.status.success() {
            return Ok(());
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        let tail: Vec<&str> = stdout
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        let tail = tail[tail.len().saturating_sub(3)..].join(" / ");
        Err(format!(
            "dism {} exited with {}: {tail}",
            args.join(" "),
            out.status
        ))
    }

    fn path_arg(name: &str, path: &Path) -> String {
        format!("/{name}:{}", path.display())
    }

    fn sized(path: &Path) -> String {
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        match fs::metadata(path) {
            Ok(m) => format!("{name} ({} KiB)", m.len().div_ceil(1024)),
            Err(_) => name,
        }
    }
}
