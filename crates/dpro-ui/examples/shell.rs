//! TD-0009 §7 U4, U6, U7: G1's probe target until roadmap 3.2 gives `dismpro.exe` its shell. The
//! synthetic shell that the benches and tests drive (390 nodes, a 40-row table over 1,000 data rows)
//! in a real window: dpro-win's window, GDI presenter and DirectWrite text, and the toolkit's frame
//! pipeline and painting. It exits with 1 if DirectWrite is unavailable rather than measure a
//! different program.
//!
//! `shell [--size WxH] [--materials auto|solid|glass] [--ui-probe first-frame|idle --probe-out
//! <file>] [--rows N] [--job-ms N] [--composites]`: the arguments of dpro-win's `bare_window`,
//! which `.github/scripts/ui-gates.ps1` measures it against, the table's data rows (G1's "Data
//! table" walks 100,000), how long the drawer's job runs from the drawer opening, its progress
//! announced (G1's "Accessibility depth: notifications"), and slice 6c's composites page instead
//! of the shell (G1's "Composites"): Ctrl+K opens the palette, Ctrl+J starts a job (its toast and
//! card), Ctrl+E ends it, and Ctrl+D opens the close-while-running dialog. It follows Windows'
//! theme, high contrast and animation settings live.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[path = "../benches/fixtures/composites.rs"]
mod composites;
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
    use std::{ffi::OsString, process::ExitCode};

    use dpro_ui::{
        Cx, Key, KeyCode, KeyEvent, Notice, Popup, SizeUnits,
        composite::ToastTone,
        materials::GlassPref,
        platform::{self, DirectWrite, Host, Model},
    };
    use dpro_win::gui::{
        Compose, WindowOptions,
        probe::{self, Args, ForceMaterials},
    };

    use crate::{
        composites as page,
        shell::{self, Msg, Shell, popup_view, view},
    };

    impl Model for Shell {
        type Msg = Msg;

        fn view(&self, cx: &mut Cx<'_, Msg>) {
            view(self, cx);
        }

        fn update(&mut self, msg: Msg) {
            Shell::update(self, msg);
        }

        fn popup(&self) -> Option<Popup<Msg>> {
            Shell::popup(self)
        }

        fn popup_view(&self, cx: &mut Cx<'_, Msg>) {
            popup_view(self, cx);
        }

        fn context_menu(&self, key: Key) -> Option<Msg> {
            Shell::context_menu(self, key)
        }

        fn size_units(&self) -> SizeUnits {
            shell::units()
        }

        fn wake(&mut self, now_ms: u64) -> Option<u64> {
            self.job_wake(now_ms)
        }

        fn notices(&mut self, f: impl FnMut(Notice<'_>)) {
            Shell::notices(self, f);
        }
    }

    /// Slice 6c's composites page (`--composites`), with its palette, toast and dialog.
    impl Model for page::Model {
        type Msg = page::Msg;

        fn view(&self, cx: &mut Cx<'_, page::Msg>) {
            page::view(self, cx);
        }

        fn update(&mut self, msg: page::Msg) {
            page::Model::update(self, msg);
        }

        fn popup(&self) -> Option<Popup<page::Msg>> {
            page::popup(self)
        }

        fn popup_view(&self, cx: &mut Cx<'_, page::Msg>) {
            page::popup_view(self, cx);
        }

        fn size_units(&self) -> SizeUnits {
            shell::units()
        }

        fn toast(&self) -> bool {
            self.over.toast()
        }

        fn toast_view(&self, cx: &mut Cx<'_, page::Msg>) {
            page::toast_view(self, cx);
        }

        /// Ctrl+K, Ctrl+J, Ctrl+E and Ctrl+D: the palette, a job's start and end, the dialog.
        fn accelerator(&self, key: KeyEvent) -> Option<page::Msg> {
            let ctrl = key.mods.ctrl && !key.mods.alt && !key.mods.shift;
            match key.key {
                KeyCode::Other(0x4B) if ctrl => Some(page::Msg::PaletteOpen),
                KeyCode::Other(0x4A) if ctrl => Some(page::Msg::JobStart),
                KeyCode::Other(0x45) if ctrl => Some(page::Msg::JobEnd(ToastTone::Done)),
                KeyCode::Other(0x44) if ctrl => Some(page::Msg::DialogOpen),
                _ => None,
            }
        }
    }

    pub(crate) fn main() -> ExitCode {
        let mut args: Vec<OsString> = std::env::args_os().skip(1).collect();
        let before = args.len();
        args.retain(|a| a != "--composites");
        let composites = args.len() != before;
        let Some((rows, job_ms, rest)) = rows(args.into_iter()) else {
            return ExitCode::from(2);
        };
        let Ok(args) = Args::parse(rest) else {
            return ExitCode::from(2);
        };
        probe::arm(&args);
        let options = WindowOptions {
            title: "DISM Pro shell probe",
            size: args.size.unwrap_or((1280, 800)),
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
        let ran = if composites {
            platform::run(
                &options,
                Host::new(page::Model::new(), Box::new(text), glass),
            )
        } else {
            let mut shell = rows.map_or_else(Shell::new, Shell::with_rows);
            if let Some(ms) = job_ms {
                shell = shell.with_job_ms(ms);
            }
            platform::run(&options, Host::new(shell, Box::new(text), glass))
        };
        match ran {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::FAILURE,
        }
    }

    /// `--rows N`, the shell's data rows, and `--job-ms N`, its drawer job's length, taken out of
    /// the arguments; the rest are the probe's. `None` if an N is missing or not a number.
    fn rows(
        mut args: impl Iterator<Item = OsString>,
    ) -> Option<(Option<usize>, Option<u64>, Vec<OsString>)> {
        let mut rows = None;
        let mut job_ms = None;
        let mut rest = Vec::new();
        while let Some(arg) = args.next() {
            if arg == "--rows" {
                rows = Some(args.next()?.to_str()?.parse().ok()?);
            } else if arg == "--job-ms" {
                job_ms = Some(args.next()?.to_str()?.parse().ok()?);
            } else {
                rest.push(arg);
            }
        }
        Some((rows, job_ms, rest))
    }
}
