//! Embeds `examples/probe.manifest` into the dpro-win examples, which are the T3 probes (see the
//! comment in that file). The library and its tests are unaffected.
#![allow(
    clippy::print_stdout,
    reason = "cargo reads build-script directives from stdout"
)]

use std::{env, path::Path};

fn main() -> Result<(), env::VarError> {
    println!("cargo:rerun-if-changed=examples/probe.manifest");
    let windows = env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows");
    let msvc = env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|e| e == "msvc");
    if !(windows && msvc) {
        return Ok(());
    }
    let manifest = Path::new(&env::var("CARGO_MANIFEST_DIR")?)
        .join("examples")
        .join("probe.manifest");
    // As in dismpro-cli's build.rs: resource 1 (RT_MANIFEST), the file's own trustInfo.
    println!("cargo:rustc-link-arg-examples=/MANIFEST:EMBED");
    println!(
        "cargo:rustc-link-arg-examples=/MANIFESTINPUT:{}",
        manifest.display()
    );
    println!("cargo:rustc-link-arg-examples=/MANIFESTUAC:NO");
    Ok(())
}
