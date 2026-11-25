//! TD-0009 A3.7's U19 and A3.9's G1 Picker check: slice 6e's path picker (`fixtures/picker.rs`)
//! in a real window, over a real folder, with dpro-win's window, GDI presenter and DirectWrite
//! text. "Browse…" asks for the system's file dialog on `--start`, as the app does; where there's
//! none (or with `--in-app`, which makes the window answer as a `WinPE` without the dialog does)
//! the in-app picker opens there instead, listed by `dpro_app::picker::FsLister` on its own
//! threads, each batch waking the window through its `Waker`; Enter on a folder's row opens it.
//! It exits with 1 if DirectWrite is unavailable rather than measure a different program.
//!
//! `picker [--start <folder>] [--in-app] [--size WxH] [--materials auto|solid|glass] [--ui-probe
//! first-frame|idle --probe-out <file>]`: the folder Browse… opens (the temporary folder by
//! default), whether the window has no system dialog, then dpro-win's probe arguments, as the
//! drawer probe takes them. `.github/scripts/ui-gates.ps1` drives it through UI Automation and the
//! keyboard: Browse… showing the system dialog, then, with `--in-app`, the in-app picker over a
//! folder of 10,000 subfolders, timing Enter on it to the grid's row count (U19).

#![cfg_attr(windows, windows_subsystem = "windows")]

#[path = "../benches/fixtures/picker.rs"]
mod picker;

use std::process::ExitCode;

fn main() -> ExitCode {
    #[cfg(windows)]
    {
        win::main()
    }
    #[cfg(not(windows))]
    {
        ExitCode::FAILURE
    }
}

#[cfg(windows)]
mod win {
    use std::{
        ffi::OsString,
        path::PathBuf,
        process::ExitCode,
        sync::{Arc, OnceLock},
    };

    use dpro_app::picker::FsLister;
    use dpro_ui::{
        Cx,
        materials::GlassPref,
        platform::{self, DirectWrite, Host, Model, Waker, file_dialog},
    };
    use dpro_win::gui::{
        Compose, WindowOptions,
        probe::{self, Args, ForceMaterials},
    };

    use crate::picker::{self, Msg};

    /// The page's view-model over the real lister, as the platform hosts it.
    #[derive(Debug)]
    struct Page {
        model: picker::Model<FsLister>,
        /// The window's waker, once the host gives it: the lister's threads wake the window
        /// with it.
        waker: Arc<OnceLock<Waker>>,
    }

    impl Model for Page {
        type Msg = Msg;

        fn view(&self, cx: &mut Cx<'_, Msg>) {
            picker::view(&self.model, cx);
        }

        fn update(&mut self, msg: Msg) {
            self.model.update(msg);
        }

        fn attach(&mut self, waker: Waker) {
            drop(self.waker.set(waker));
        }

        /// After every event: the batches that came (a wake brings them), taken into the model.
        fn wake(&mut self, _now_ms: u64) -> Option<u64> {
            self.model.update(Msg::Listed);
            None
        }

        /// Browse…'s request for the system dialog; its answer comes back as `Msg::Picked`.
        fn picks(&mut self, f: impl FnMut(&file_dialog::Pick<'_>, fn(file_dialog::Picked) -> Msg)) {
            self.model.picks(f);
        }
    }

    /// `--start <folder>`, taken out of `args`; the rest are the probe's.
    fn start(args: &mut Vec<OsString>) -> Option<Result<PathBuf, ()>> {
        let at = args.iter().position(|a| a == "--start")?;
        args.remove(at);
        Some(if at < args.len() {
            Ok(PathBuf::from(args.remove(at)))
        } else {
            Err(())
        })
    }

    /// `--in-app`, taken out of `args`.
    fn in_app(args: &mut Vec<OsString>) -> bool {
        let at = args.iter().position(|a| a == "--in-app");
        at.map(|at| args.remove(at)).is_some()
    }

    pub(crate) fn main() -> ExitCode {
        let mut args: Vec<OsString> = std::env::args_os().skip(1).collect();
        let start = match start(&mut args) {
            Some(Ok(folder)) => folder,
            Some(Err(())) => return ExitCode::from(2),
            None => std::env::temp_dir(),
        };
        let in_app = in_app(&mut args);
        let Ok(args) = Args::parse(args) else {
            return ExitCode::from(2);
        };
        probe::arm(&args);
        let options = WindowOptions {
            title: "DISM Pro picker probe",
            size: args.size.unwrap_or((1000, 700)),
            min_size: (500, 400),
            // `platform::run` decides.
            compose: Compose::Never,
        };
        let Ok(text) = DirectWrite::new() else {
            return ExitCode::FAILURE;
        };
        let glass = match args.materials {
            ForceMaterials::Auto => GlassPref::Auto,
            ForceMaterials::Solid => GlassPref::Off,
            ForceMaterials::Glass => GlassPref::Force,
        };
        let waker = Arc::new(OnceLock::<Waker>::new());
        let for_lister = Arc::clone(&waker);
        let lister = FsLister::new(Box::new(move || {
            if let Some(w) = for_lister.get() {
                w.wake();
            }
        }));
        let page = Page {
            model: picker::Model::with_lister(lister, start),
            waker,
        };
        let mut host = Host::new(page, Box::new(text), glass);
        if in_app {
            host = host.without_file_dialog();
        }
        match platform::run(&options, host) {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::FAILURE,
        }
    }
}
