//! `dismpro.exe`: the GUI and the approval UI (`--approval`, ADR-0006).
//!
//! Layer: binary. May depend on any workspace crate except `dpro-sys` (go through `dpro-win`).

#![forbid(unsafe_code)]
// No console window for the GUI; ignored on non-Windows targets.
#![windows_subsystem = "windows"]

fn main() {}
