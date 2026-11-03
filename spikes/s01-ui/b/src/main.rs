//! Spike S-01 option B (roadmap 1.6, ADR-0002): the three screens in Slint, software renderer.
//!
//! ```text
//! s01-b --snapshot <dir> [--scales 1,1.5]           headless snapshots + pixel checks (exit 1 on a failure)
//! s01-b --bench [--scales 1,1.5] [--frames <file>]  headless 100k-list fling script
//! s01-b [--screen shell|list|drawer] [--theme light|dark] [--size WxH] [--glass] [--first-frame <file>]
//!                                                   a window (Windows: winit + AccessKit)
//! ```

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::{error::Error, path::Path, process::ExitCode};

/// The components Slint generates from `ui/app.slint`. Generated code isn't ours to lint (it unwraps
/// internally), so the allow covers this module only.
#[allow(clippy::all, clippy::pedantic, clippy::unwrap_used)]
mod ui {
    slint::include_modules!();
}
use ui::{App, Area, Column, Item, NavEntry, Palette, Row};

mod headless;
mod model;
#[cfg(windows)]
mod win;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match run(&args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("s01-b: {e}");
            ExitCode::FAILURE
        }
    }
}

fn scales(args: &[String]) -> Vec<f32> {
    probe::value(args, "--scales").map_or_else(
        || vec![1.0, 1.5],
        |s| s.split(',').filter_map(|v| v.parse().ok()).collect(),
    )
}

fn run(args: &[String]) -> Result<ExitCode, Box<dyn Error>> {
    if let Some(dir) = probe::value(args, "--snapshot") {
        let failures = headless::snapshot(Path::new(&dir), &scales(args))?;
        return Ok(if failures == 0 {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        });
    }
    if probe::flag(args, "--bench") {
        let frames = probe::value(args, "--frames");
        headless::bench(&scales(args), frames.as_deref().map(Path::new))?;
        return Ok(ExitCode::SUCCESS);
    }
    #[cfg(windows)]
    {
        win::run(args)?;
        Ok(ExitCode::SUCCESS)
    }
    #[cfg(not(windows))]
    Err("the window mode needs Windows; use --snapshot or --bench".into())
}
