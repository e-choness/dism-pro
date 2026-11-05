//! TD-0009 §7 U4, U6, U7: G1's probe target until roadmap 3.2 gives `dismpro.exe` its shell. The
//! synthetic shell that the benches and tests drive (387 nodes, a 40-row table over 1,000 data rows)
//! in a real window: dpro-win's window, GDI presenter and DirectWrite text, and the toolkit's frame
//! pipeline and painting. It exits with 1 if DirectWrite is unavailable rather than measure a
//! different program.
//!
//! `shell [--size WxH] [--ui-probe first-frame|idle --probe-out <file>]`, the arguments of
//! dpro-win's `bare_window`, which `.github/scripts/ui-gates.ps1` measures it against.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[path = "../benches/fixtures/shell.rs"]
mod shell;

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
        Cx, Motion, Theme,
        platform::{self, DirectWrite, Host, Model},
        tokens::motion::HOVER_MS,
    };
    use dpro_win::gui::{
        WindowOptions,
        probe::{self, Args},
    };

    use crate::shell::{Msg, Shell, view};

    impl Model for Shell {
        type Msg = Msg;

        fn view(&self, cx: &mut Cx<'_, Msg>) {
            view(self, cx);
        }

        fn update(&mut self, msg: Msg) {
            Shell::update(self, msg);
        }
    }

    pub(crate) fn main() -> ExitCode {
        let Ok(args) = Args::parse(std::env::args_os().skip(1)) else {
            return ExitCode::from(2);
        };
        probe::arm(&args);
        let options = WindowOptions {
            title: "DISM Pro shell probe",
            size: args.size.unwrap_or((1280, 800)),
            min_size: (500, 400),
        };
        let Ok(text) = DirectWrite::new() else {
            return ExitCode::FAILURE;
        };
        let host = Host::new(
            Shell::new(),
            Box::new(text),
            Theme::LIGHT,
            Motion { hover_ms: HOVER_MS },
        );
        match platform::run(&options, host) {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::FAILURE,
        }
    }
}
