//! `cargo xtask pe-compat --target <triple>…`: fails when a release exe imports (or
//! delay-loads) a DLL outside the `WinPE` allowlist in `xtask/data/winpe-dlls.txt`.
//! The substitute for a `WinPE` VM (doc 04 §4).

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use crate::{
    Result, pe,
    size::{parse_targets, shipped},
};

const ALLOWLIST: &str = include_str!("../data/winpe-dlls.txt");

pub(crate) fn run(root: &Path, args: &[String]) -> Result<()> {
    let targets = parse_targets(args)?;
    let target_dir =
        env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from);
    let allowlist = parse_allowlist(ALLOWLIST);

    let mut checked = 0_usize;
    let mut failures = 0_usize;
    for target in &targets {
        let release = target_dir.join(target).join("release");
        for name in shipped() {
            let label = format!("{target}/{name}");
            let Ok(bytes) = fs::read(release.join(name)) else {
                return Err(format!("pe-compat: {label} not found (build first)").into());
            };
            let imports = pe::imports(&bytes).map_err(|e| format!("{label}: {e}"))?;
            let outside = rejected(&imports, &allowlist);
            checked += 1;
            if outside.is_empty() {
                println!("pe-compat: ok   {label} ({} DLLs)", imports.len());
                continue;
            }
            failures += 1;
            println!("pe-compat: FAIL {label} imports DLLs outside the WinPE allowlist:");
            for dll in &outside {
                println!("    {dll}");
            }
            let all: Vec<&str> = imports.iter().map(|i| i.dll.as_str()).collect();
            println!("  all imports: {}", all.join(", "));
        }
    }
    if checked == 0 {
        return Err("pe-compat: no release executables found (build first)".into());
    }
    if failures > 0 {
        let hint = "add a DLL to xtask/data/winpe-dlls.txt only with a cited source";
        return Err(format!("{failures} executable(s) not WinPE-compatible; {hint}").into());
    }
    Ok(())
}

fn parse_allowlist(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| {
            l.split('#')
                .next()
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase()
        })
        .filter(|l| !l.is_empty())
        .collect()
}

/// Imported DLL names (with a `(delay-load)` note) that no allowlist entry matches.
fn rejected(imports: &[pe::Import], allowlist: &[String]) -> Vec<String> {
    imports
        .iter()
        .filter(|i| !is_allowed(&i.dll, allowlist))
        .map(|i| {
            let note = if i.delay_loaded { " (delay-load)" } else { "" };
            format!("{}{note}", i.dll)
        })
        .collect()
}

fn is_allowed(dll: &str, allowlist: &[String]) -> bool {
    let dll = dll.to_ascii_lowercase();
    allowlist.iter().any(|entry| match entry.strip_suffix('*') {
        Some(prefix) => dll.starts_with(prefix),
        None => dll == *entry,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_file_parses() {
        let list = parse_allowlist(ALLOWLIST);
        assert!(list.contains(&"kernel32.dll".to_owned()));
        assert!(list.iter().all(|e| !e.contains(char::is_whitespace)));
    }

    #[test]
    fn matching_is_case_insensitive_with_prefix_wildcards() {
        let list = parse_allowlist("kernel32.dll\napi-ms-win-core-*  # api sets\n");
        assert!(is_allowed("KERNEL32.dll", &list));
        assert!(is_allowed("api-ms-win-core-synch-l1-2-0.dll", &list));
        assert!(!is_allowed("vcruntime140.dll", &list));
    }

    #[test]
    fn reports_rejected_imports_from_a_real_pe_layout() {
        let bytes = pe::tests::synthetic_pe(&["KERNEL32.dll", "VCRUNTIME140.dll"], &["d3d11.dll"]);
        let imports = pe::imports(&bytes).unwrap();
        let list = parse_allowlist("kernel32.dll");
        let expected = ["VCRUNTIME140.dll", "d3d11.dll (delay-load)"];
        assert_eq!(rejected(&imports, &list), expected);
    }
}
