//! Prints the S-03 header cross-check C source for one header: used by `cargo xtask ffi-check`.
//! Usage: `cargo run -p dpro-sys --example ffi-check-source -- dismapi.h`

#![forbid(unsafe_code)]
#![allow(clippy::print_stdout)]

use std::process::ExitCode;

use dpro_sys::layout::{HEADERS, c_check_source};

fn main() -> ExitCode {
    let wanted = std::env::args().nth(1).unwrap_or_default();
    match HEADERS.iter().find(|h| h.file == wanted) {
        Some(header) => {
            print!("{}", c_check_source(header));
            ExitCode::SUCCESS
        }
        None => {
            let known: Vec<&str> = HEADERS.iter().map(|h| h.file).collect();
            eprintln!("usage: ffi-check-source <{}>", known.join("|"));
            ExitCode::FAILURE
        }
    }
}
