//! TD-0009 A3.7's U16 and A3.9's G1 Drawer check: slice 6d's drawer page (`fixtures/drawer.rs`) in
//! a real window, with dpro-win's window, GDI presenter and DirectWrite text. "Review changes"
//! opens the drawer, which slides in; Esc closes it and focus goes back to the opener. Delete
//! drops the first change still listed, as a refresh would: focus goes to its successor (A4.9's
//! G1 focus checks, slice 7c). It exits with 1 if DirectWrite is unavailable rather than measure
//! a different program.
//!
//! `drawer [--size WxH] [--materials auto|solid|glass] [--ui-probe first-frame|idle --probe-out
//! <file>]`: dpro-win's probe arguments, as the shell probe takes them. `.github/scripts/
//! ui-gates.ps1` drives it through UI Automation and the keyboard, and counts the slide's frame
//! timers with `--ui-probe idle`.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[path = "../benches/fixtures/drawer.rs"]
mod drawer;

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
    use std::process::ExitCode;

    use dpro_ui::{
        Cx, KeyCode, KeyEvent, Mods,
        materials::GlassPref,
        platform::{self, DirectWrite, Host, Model},
    };
    use dpro_win::gui::{
        Compose, WindowOptions,
        probe::{self, Args, ForceMaterials},
    };

    use crate::drawer::{self, Msg};

    /// The page's view-model, as the platform hosts it.
    #[derive(Debug)]
    struct Page(drawer::Model);

    impl Model for Page {
        type Msg = Msg;

        fn view(&self, cx: &mut Cx<'_, Msg>) {
            drawer::view(&self.0, cx);
        }

        fn update(&mut self, msg: Msg) {
            self.0.update(msg);
        }

        /// Delete drops the first change still listed (TD-0009 A4.9, slice 7c).
        fn accelerator(&self, key: KeyEvent) -> Option<Msg> {
            (key.key == KeyCode::Delete && key.mods == Mods::default()).then_some(Msg::Drop)
        }
    }

    pub(crate) fn main() -> ExitCode {
        let Ok(args) = Args::parse(std::env::args_os().skip(1)) else {
            return ExitCode::from(2);
        };
        probe::arm(&args);
        let options = WindowOptions {
            title: "DISM Pro drawer probe",
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
        let host = Host::new(Page(drawer::Model::new()), Box::new(text), glass);
        match platform::run(&options, host) {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::FAILURE,
        }
    }
}
