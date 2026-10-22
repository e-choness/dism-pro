//! Repository automation: `cargo xtask <task>`. Runs inside the toolchain container only (ADR-0003).
//!
//! Layer: tooling. No workspace dependencies.

#![forbid(unsafe_code)]
// xtask reports to the terminal.
#![allow(clippy::print_stdout)]

mod i18n;
mod layering;
mod manifest;
mod size;

use std::{
    env,
    path::{Path, PathBuf},
    process::ExitCode,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const USAGE: &str = "usage: cargo xtask <task>
  layering                     crate layering + unsafe rules (doc 02 §2)
  i18n-check [--release]       Fluent catalog consistency (ADR-0005)
  size --target <triple>...    exact binary-size budgets (doc 04 §2)";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<()> {
    let root = workspace_root();
    let Some((task, rest)) = args.split_first() else {
        return Err(USAGE.into());
    };
    match task.as_str() {
        "layering" => layering::run(&root),
        "i18n-check" => i18n::run(&root, rest),
        "size" => size::run(&root, rest),
        _ => Err(USAGE.into()),
    }
}

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is `<root>/xtask`.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    dir.parent().unwrap_or(dir).to_path_buf()
}
