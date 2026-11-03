//! `cargo xtask pe-compat --target <triple>…`: fails when a release exe imports (or
//! delay-loads) a DLL outside the `WinPE` allowlist in `xtask/data/winpe-dlls.txt`.
//! The substitute for a `WinPE` VM (doc 04 §4). With `--winpe-files <dir>` it checks the allowlist
//! itself against the DLLs base `WinPE` ships.

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
    if let [flag, dir] = args
        && flag == "--winpe-files"
    {
        return verify_allowlist(Path::new(dir), &parse_allowlist(ALLOWLIST));
    }
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

/// `pe-compat --winpe-files <dir>`: the evidence check for the allowlist itself. `<dir>` holds one
/// `<arch>.txt` per architecture with the DLL names in base `WinPE`'s `System32` (written in CI by
/// `.github/scripts/adk.ps1` from the ADK's `winpe.wim`). Every entry must exist in every
/// architecture; API sets (`api-ms-*`, `ext-ms-*`) are virtual names the loader resolves, so
/// they're skipped.
fn verify_allowlist(dir: &Path, allowlist: &[String]) -> Result<()> {
    let mut lists = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "txt") {
            let arch = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let names: Vec<String> = fs::read_to_string(&path)?
                .lines()
                .map(|l| l.trim().to_ascii_lowercase())
                .filter(|l| !l.is_empty())
                .collect();
            lists.push((arch, names));
        }
    }
    if lists.is_empty() {
        return Err(format!(
            "pe-compat: no <arch>.txt WinPE file lists in {}",
            dir.display()
        )
        .into());
    }
    lists.sort();
    let missing = missing_from_winpe(allowlist, &lists);
    for (arch, names) in &lists {
        println!("pe-compat: WinPE {arch}: {} DLLs listed", names.len());
    }
    if missing.is_empty() {
        println!("pe-compat: ok   every allowlist entry ships in base WinPE");
        return Ok(());
    }
    let text = missing.join(", ");
    crate::annotate("WinPE allowlist", &format!("not in base WinPE: {text}"));
    Err(format!("pe-compat: allowlist entries missing from base WinPE: {text}").into())
}

/// Allowlist entries (with the architecture) that no DLL in that `WinPE` file list matches.
fn missing_from_winpe(allowlist: &[String], lists: &[(String, Vec<String>)]) -> Vec<String> {
    let mut missing = Vec::new();
    for entry in allowlist {
        if entry.starts_with("api-ms-") || entry.starts_with("ext-ms-") {
            continue;
        }
        for (arch, names) in lists {
            if !names
                .iter()
                .any(|n| is_allowed(n, std::slice::from_ref(entry)))
            {
                missing.push(format!("{entry} ({arch})"));
            }
        }
    }
    missing
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
    fn allowlist_entries_must_ship_in_every_winpe_architecture() {
        let allowlist = parse_allowlist("kernel32.dll\ndismapi.dll\nbcrypt*\napi-ms-win-core-*\n");
        let lists = [
            (
                "amd64".to_owned(),
                vec![
                    "kernel32.dll".to_owned(),
                    "dismapi.dll".to_owned(),
                    "bcryptprimitives.dll".to_owned(),
                ],
            ),
            (
                "arm64".to_owned(),
                vec!["kernel32.dll".to_owned(), "bcrypt.dll".to_owned()],
            ),
        ];
        assert_eq!(
            missing_from_winpe(&allowlist, &lists),
            ["dismapi.dll (arm64)"]
        );
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
