//! Embeds `dismpro-cli.manifest` into the Windows executable (see the comment in that file).
#![allow(
    clippy::print_stdout,
    reason = "cargo reads build-script directives from stdout"
)]

use std::{env, path::Path};

fn main() -> Result<(), env::VarError> {
    println!("cargo:rerun-if-changed=dismpro-cli.manifest");
    let windows = env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows");
    let msvc = env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|e| e == "msvc");
    if !(windows && msvc) {
        return Ok(());
    }
    let manifest = Path::new(&env::var("CARGO_MANIFEST_DIR")?).join("dismpro-cli.manifest");
    // link.exe and lld-link both embed the file as resource 1 (RT_MANIFEST); the manifest
    // carries its own trustInfo, so the linker's default UAC fragment is turned off.
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!(
        "cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}",
        manifest.display()
    );
    println!("cargo:rustc-link-arg-bins=/MANIFESTUAC:NO");
    Ok(())
}
