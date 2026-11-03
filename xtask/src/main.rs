//! Repository automation: `cargo xtask <task>`. Runs inside the toolchain container only (ADR-0003).
//!
//! Layer: tooling. No workspace dependencies.

#![forbid(unsafe_code)]
// xtask reports to the terminal.
#![allow(clippy::print_stdout)]

mod ffi_check;
mod i18n;
mod layering;
mod manifest;
mod pe;
mod pe_compat;
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
  size --target <triple>...    exact binary-size budgets (doc 04 §2)
  pe-compat --target <triple>...  WinPE DLL import allowlist (doc 04 §4)
  pe-compat --winpe-files <dir>   allowlist vs DLLs in base WinPE (CI evidence)
  ffi-check                    dpro-sys layouts/prototypes vs SDK/ADK headers (S-03)";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask: {e}");
            let task = args.first().map_or("", String::as_str);
            annotate(&format!("xtask {task}"), &e.to_string());
            ExitCode::FAILURE
        }
    }
}

/// Under GitHub Actions, prints an error annotation: shown on the PR, and readable through the
/// checks API without the job log. Does nothing elsewhere. Compose passes `GITHUB_ACTIONS` in.
pub(crate) fn annotate(title: &str, message: &str) {
    if env::var_os("GITHUB_ACTIONS").is_some_and(|v| v == "true") {
        println!("{}", annotation(title, message));
    }
}

/// `::error title=…::…` with the workflow-command escaping.
fn annotation(title: &str, message: &str) -> String {
    let data = |s: &str| {
        s.replace('%', "%25")
            .replace('\r', "%0D")
            .replace('\n', "%0A")
    };
    let title = data(title).replace(':', "%3A").replace(',', "%2C");
    format!("::error title={title}::{}", data(message))
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
        "pe-compat" => pe_compat::run(&root, rest),
        "ffi-check" => ffi_check::run(&root, rest),
        _ => Err(USAGE.into()),
    }
}

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is `<root>/xtask`.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    dir.parent().unwrap_or(dir).to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annotations_escape_workflow_command_syntax() {
        assert_eq!(
            annotation("S-03 a.h (x86_64), arm", "50% off\r\nline 2"),
            "::error title=S-03 a.h (x86_64)%2C arm::50%25 off%0D%0Aline 2"
        );
        assert_eq!(
            annotation("xtask: size", "ok"),
            "::error title=xtask%3A size::ok"
        );
    }
}
