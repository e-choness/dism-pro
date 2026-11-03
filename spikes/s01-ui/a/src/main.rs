//! Spike S-01 option A (roadmap 1.6, ADR-0002): a custom toolkit.
//!
//! Every frame is drawn in software (tiny-skia shapes, DirectWrite text: both work in base WinPE).
//! Presentation is GDI in solid mode, or DirectComposition with premultiplied alpha for the glass
//! materials (ADR-0007), with d3d11/dxgi/dcomp/d2d1 resolved at run time so the exe still starts
//! in WinPE. UI Automation comes from AccessKit.
//!
//! ```text
//! s01-a --snapshot <dir> [--scales 1,1.5]            headless snapshots + pixel checks (exit 1 on a failure)
//! s01-a --bench [--scales 1,1.5] [--frames <file>]   headless 100k-list fling script
//! s01-a --acrylic-bench [--rounds N]                 in-app acrylic cost: software, and Direct2D on WARP
//! s01-a [--screen shell|list|drawer] [--theme light|dark] [--size WxH] [--at X,Y] [--solid]
//!       [--palette] [--palette-at X,Y] [--first-frame <file>]
//!                                                    a window (glass follows Windows' transparency)
//! ```

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod a11y;
#[cfg(windows)]
mod acrylic;
#[cfg(windows)]
mod canvas;
#[cfg(windows)]
mod draw;
#[cfg(windows)]
mod gpu;
#[cfg(windows)]
mod headless;
#[cfg(windows)]
mod text;
#[cfg(windows)]
mod win;

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match run(&args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("s01-a: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(windows)]
fn scales(args: &[String]) -> Vec<f32> {
    probe::value(args, "--scales").map_or_else(
        || vec![1.0, 1.5],
        |s| s.split(',').filter_map(|v| v.parse().ok()).collect(),
    )
}

#[cfg(windows)]
fn run(args: &[String]) -> Result<std::process::ExitCode, Box<dyn std::error::Error>> {
    use std::{path::Path, process::ExitCode};
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
    if probe::flag(args, "--acrylic-bench") {
        let rounds = probe::value(args, "--rounds")
            .and_then(|v| v.parse().ok())
            .unwrap_or(31);
        headless::acrylic_bench(rounds)?;
        return Ok(ExitCode::SUCCESS);
    }
    win::run(args)?;
    Ok(ExitCode::SUCCESS)
}

/// Option A is Windows-only (DirectWrite); C1 runs the Windows build under Wine.
#[cfg(not(windows))]
fn main() {}
