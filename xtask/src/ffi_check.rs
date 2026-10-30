//! `cargo xtask ffi-check`: the S-03 header cross-check (ADR-0001).
//!
//! For each header mirrored by `dpro-sys` it generates C static asserts (via the
//! `ffi-check-source` example) and compiles them with `clang-cl /Zs` for x64 and ARM64
//! against Microsoft's headers. A mismatch in any size, offset or prototype fails the build.
//!
//! Header search path: the Windows SDK baked into the image (`$XWIN_CACHE_DIR`), plus any
//! directories in `DPRO_FFI_INCLUDE` (`:`-separated), e.g. the ADK's `SDKs/DismApi/Include`.
//! A header that can't be found is an error, never a skip.

use std::{
    env,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use crate::Result;

const HEADERS: [&str; 2] = ["dismapi.h", "wimgapi.h"];
const TARGETS: [&str; 2] = ["x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc"];

pub(crate) fn run(root: &Path, args: &[String]) -> Result<()> {
    if !args.is_empty() {
        return Err("ffi-check takes no arguments".into());
    }
    let mut includes = sdk_include_dirs()?;
    if let Some(extra) = env::var_os("DPRO_FFI_INCLUDE") {
        includes.extend(env::split_paths(&extra));
    }
    let out_dir = root.join("target").join("ffi-check");
    fs::create_dir_all(&out_dir)?;

    let mut failures = Vec::new();
    for header in HEADERS {
        if find_header(&includes, header).is_none() {
            failures.push(format!("{header}: not found (set DPRO_FFI_INCLUDE)"));
            continue;
        }
        let source = generate(header)?;
        let file = out_dir.join(format!("{header}.c"));
        fs::write(&file, source)?;
        for target in TARGETS {
            let out = Command::new("clang-cl")
                .args(["/nologo", "/Zs", "/TC", "-Wno-unused-command-line-argument"])
                .arg(format!("--target={target}"))
                .args(
                    includes
                        .iter()
                        .flat_map(|d| [PathBuf::from("/imsvc"), d.clone()]),
                )
                .arg(&file)
                .output()?;
            let diagnostics = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            print!("{diagnostics}");
            if out.status.success() {
                println!("ffi-check: ok   {header} ({target})");
            } else {
                crate::annotate(
                    &format!("S-03 {header} ({target})"),
                    &error_lines(&diagnostics),
                );
                failures.push(format!(
                    "{header} ({target}): mismatch, see clang-cl output"
                ));
            }
        }
    }
    if failures.is_empty() {
        return Ok(());
    }
    for f in &failures {
        println!("ffi-check: FAIL {f}");
    }
    Err(format!("{} ffi-check failure(s) (S-03)", failures.len()).into())
}

/// The compiler's `error:` lines (paths trimmed to the file name), capped so one annotation stays
/// readable; the full output is in the log.
fn error_lines(diagnostics: &str) -> String {
    const MAX: usize = 40;
    let errors: Vec<String> = diagnostics
        .lines()
        .filter_map(|l| {
            let (location, message) = l.split_at(l.find("error:")?);
            let file = location.rsplit_once('/').map_or(location, |(_, f)| f);
            Some(format!("{file}{message}"))
        })
        .collect();
    if errors.is_empty() {
        let lines: Vec<&str> = diagnostics.lines().collect();
        return lines[lines.len().saturating_sub(10)..].join("\n");
    }
    let mut text = errors[..errors.len().min(MAX)].join("\n");
    if errors.len() > MAX {
        let _ = write!(text, "\n… {} more", errors.len() - MAX); // ignore: String writes can't fail
    }
    text
}

fn generate(header: &str) -> Result<String> {
    let out = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .args([
            "run",
            "--quiet",
            "--locked",
            "-p",
            "dpro-sys",
            "--example",
            "ffi-check-source",
        ])
        .arg("--")
        .arg(header)
        .output()?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("generating {header}.c failed: {err}").into());
    }
    Ok(String::from_utf8(out.stdout)?)
}

/// `crt/include` and `sdk/include/{ucrt,um,shared}` of the xwin splat, wherever
/// cargo-xwin put it under `$XWIN_CACHE_DIR`.
fn sdk_include_dirs() -> Result<Vec<PathBuf>> {
    let base =
        env::var_os("XWIN_CACHE_DIR").map_or_else(|| PathBuf::from("/opt/xwin"), PathBuf::from);
    let root = find_splat_root(&base, 3)
        .ok_or_else(|| format!("no xwin SDK splat under {}", base.display()))?;
    let sdk = root.join("sdk").join("include");
    Ok(vec![
        root.join("crt").join("include"),
        sdk.join("ucrt"),
        sdk.join("um"),
        sdk.join("shared"),
    ])
}

fn find_splat_root(dir: &Path, depth: u32) -> Option<PathBuf> {
    if dir.join("crt").join("include").is_dir()
        && dir.join("sdk").join("include").join("um").is_dir()
    {
        return Some(dir.to_path_buf());
    }
    if depth == 0 {
        return None;
    }
    let mut children: Vec<PathBuf> = fs::read_dir(dir)
        .ok()?
        .filter_map(|e| Some(e.ok()?.path()))
        .collect();
    children.sort();
    children
        .into_iter()
        .filter(|p| p.is_dir())
        .find_map(|p| find_splat_root(&p, depth - 1))
}

/// Case-insensitive lookup: SDK header names vary in case (`DismApi.h`).
fn find_header(dirs: &[PathBuf], name: &str) -> Option<PathBuf> {
    dirs.iter().find_map(|dir| {
        fs::read_dir(dir)
            .ok()?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .find(|p| {
                p.file_name()
                    .is_some_and(|f| f.to_string_lossy().eq_ignore_ascii_case(name))
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("xtask-ffi-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir); // ignore: may not exist
        dir
    }

    #[test]
    fn finds_the_splat_root_below_the_cache_dir() {
        let base = scratch("splat");
        let root = base.join("xwin");
        fs::create_dir_all(root.join("crt/include")).unwrap();
        fs::create_dir_all(root.join("sdk/include/um")).unwrap();
        assert_eq!(find_splat_root(&base, 3), Some(root));
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn error_lines_keep_messages_and_drop_directories() {
        let out = "/src/target/ffi-check/dismapi.h.c(7,1): error: static assertion failed: a/b\n\
                   note: expanded from here\n\
                   /x/um/dismapi.h(3,9): error: unknown type name 'DISM'\n";
        assert_eq!(
            error_lines(out),
            "dismapi.h.c(7,1): error: static assertion failed: a/b\n\
             dismapi.h(3,9): error: unknown type name 'DISM'"
        );
        assert_eq!(error_lines("one\ntwo"), "one\ntwo");
    }

    #[test]
    fn header_lookup_ignores_case() {
        let dir = scratch("hdr");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("DismApi.h"), "").unwrap();
        let dirs = [dir.clone()];
        assert_eq!(find_header(&dirs, "dismapi.h"), Some(dir.join("DismApi.h")));
        assert_eq!(find_header(&dirs, "wimgapi.h"), None);
        fs::remove_dir_all(&dir).unwrap();
    }
}
