// Included by each S-01 exe's build.rs: embeds the shared `gui.manifest` (see the comment there).

/// Writes `gui.manifest` with the target's architecture to `OUT_DIR` and embeds it. link.exe and
/// lld-link both embed the file as resource 1 (`RT_MANIFEST`); the manifest carries its own
/// trustInfo, so the linker's default UAC fragment is turned off.
fn embed_manifest() {
    use std::{env, fs, path::Path, process};

    let (Ok(dir), Ok(out)) = (env::var("CARGO_MANIFEST_DIR"), env::var("OUT_DIR")) else {
        return;
    };
    let source = Path::new(&dir).join("..").join("gui.manifest");
    println!("cargo:rerun-if-changed={}", source.display());
    let windows = env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows");
    let msvc = env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|e| e == "msvc");
    if !(windows && msvc) {
        return;
    }
    let arch = match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86_64") => "amd64",
        Ok("aarch64") => "arm64",
        Ok("x86") => "x86",
        other => {
            eprintln!("manifest: no Windows architecture name for {other:?}");
            process::exit(1);
        }
    };
    let text = match fs::read_to_string(&source) {
        Ok(t) => t.replace(
            "processorArchitecture=\"*\"",
            &format!("processorArchitecture=\"{arch}\""),
        ),
        Err(e) => {
            eprintln!("manifest: {}: {e}", source.display());
            process::exit(1);
        }
    };
    let manifest = Path::new(&out).join("gui.manifest");
    if let Err(e) = fs::write(&manifest, text) {
        eprintln!("manifest: {}: {e}", manifest.display());
        process::exit(1);
    }
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", manifest.display());
    println!("cargo:rustc-link-arg-bins=/MANIFESTUAC:NO");
}
