//! `dismpro-cli.exe`: CLI, MCP front (`mcp`) and headless agent broker (`--agent-broker`).
//!
//! Layer: binary. May depend on any workspace crate except `dpro-sys` (go through `dpro-win`).
//! Implements TD-0002; health commands per TD-0006.

#![forbid(unsafe_code)]
// A CLI prints.
#![allow(clippy::print_stdout, clippy::print_stderr)]
// Most commands are Windows-only; their helpers are unused when building for Linux (T1).
#![cfg_attr(not(windows), allow(dead_code))]

mod args;
mod error;
mod json;
mod out;
#[cfg(windows)]
mod run;

use std::path::Path;

use args::{Cli, Command, JournalRef};
use dpro_cli_fmt::{exit::ExitCode, messages::Messages};
use error::CliError;

fn main() -> std::process::ExitCode {
    let m = Messages::en_us();
    let argv: Vec<_> = std::env::args_os().skip(1).collect();
    let cli = match args::parse(argv.clone()) {
        Ok(cli) => cli,
        Err(e) => {
            // Honour --json even when the rest of the command line is wrong.
            let json = argv.iter().any(|a| a == "--json");
            return out::report_error(&CliError::Usage(e.0), json, &m);
        }
    };
    match dispatch(&cli, &m) {
        Ok(code) => std::process::ExitCode::from(code.code()),
        Err(e) => out::report_error(&e, cli.global.json, &m),
    }
}

fn dispatch(cli: &Cli, m: &Messages) -> Result<ExitCode, CliError> {
    match &cli.command {
        Command::Help => {
            println!("{}\n\n{}", m.text("cli-usage"), m.text("cli-help"));
            Ok(ExitCode::Ok)
        }
        Command::Version => {
            println!(
                "{}",
                m.get("cli-version", &[("version", &env!("CARGO_PKG_VERSION"))])
            );
            Ok(ExitCode::Ok)
        }
        Command::JournalShow(JournalRef::File(path)) => journal_show(cli, m, path),
        _ => platform(cli, m),
    }
}

#[cfg(windows)]
fn platform(cli: &Cli, m: &Messages) -> Result<ExitCode, CliError> {
    run::run(cli, m)
}

#[cfg(not(windows))]
fn platform(_cli: &Cli, _m: &Messages) -> Result<ExitCode, CliError> {
    Err(CliError::Unsupported)
}

/// `journal show <file>`: reading a journal file needs no OS access, so it works anywhere.
pub(crate) fn journal_show(cli: &Cli, m: &Messages, path: &Path) -> Result<ExitCode, CliError> {
    let bytes = std::fs::read(path).map_err(|e| CliError::io_at(path, &e))?;
    let rec = dpro_engine::journal::read(&bytes)?;
    if cli.global.json {
        out::print_json(&out::job_doc(&rec, path))?;
    } else {
        out::print_job(&rec, path, m);
    }
    Ok(ExitCode::Ok)
}
