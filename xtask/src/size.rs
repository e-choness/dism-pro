//! `cargo xtask size --target <triple>…`: exact binary-size gate (doc 04 §2, §3).
//! Reads release exes from `target/<triple>/release/`, so run it after the build.

use std::{
    env, fmt, fs,
    path::{Path, PathBuf},
};

use crate::Result;

const MIB: u64 = 1024 * 1024;

/// Executable and its budget. GUI ≤ 6 MiB, CLI ≤ 4 MiB (doc 04 §2).
const BUDGETS: [(&str, Bytes); 2] = [
    ("dismpro.exe", Bytes(6 * MIB)),
    ("dismpro-cli.exe", Bytes(4 * MIB)),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Bytes(u64);

impl fmt::Display for Bytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} B ({} KiB)", self.0, self.0.div_ceil(1024))
    }
}

pub(crate) fn run(root: &Path, args: &[String]) -> Result<()> {
    let targets = parse_targets(args)?;
    let target_dir =
        env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from);

    let mut failures = 0_usize;
    for target in &targets {
        for (exe, budget) in BUDGETS {
            let path = target_dir.join(target).join("release").join(exe);
            let Ok(meta) = fs::metadata(&path) else {
                println!("size: FAIL {target}/{exe}: not found (build first)");
                failures += 1;
                continue;
            };
            let size = Bytes(meta.len());
            let ok = within(size, budget);
            if !ok {
                failures += 1;
            }
            let verdict = if ok { "ok  " } else { "FAIL" };
            println!("size: {verdict} {target}/{exe}: {size} / budget {budget}");
        }
    }
    if failures > 0 {
        return Err(format!("{failures} executable(s) over budget or missing").into());
    }
    Ok(())
}

/// The executables a release ships (what `pe-compat` checks too).
pub(crate) fn shipped() -> impl Iterator<Item = &'static str> {
    BUDGETS.iter().map(|(exe, _)| *exe)
}

fn within(size: Bytes, budget: Bytes) -> bool {
    size <= budget
}

pub(crate) fn parse_targets(args: &[String]) -> Result<Vec<String>> {
    let mut targets = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match (arg.as_str(), it.next()) {
            ("--target", Some(t)) => targets.push(t.clone()),
            _ => return Err(format!("unexpected argument {arg:?}").into()),
        }
    }
    if targets.is_empty() {
        return Err("pass at least one --target <triple>".into());
    }
    Ok(targets)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn budget_is_inclusive() {
        assert!(within(Bytes(4 * MIB), Bytes(4 * MIB)));
        assert!(!within(Bytes(4 * MIB + 1), Bytes(4 * MIB)));
    }

    #[test]
    fn parses_targets() {
        let a = args(&["--target", "x86_64-pc-windows-msvc", "--target", "b"]);
        assert_eq!(parse_targets(&a).unwrap(), ["x86_64-pc-windows-msvc", "b"]);
        assert!(parse_targets(&args(&[])).is_err());
        assert!(parse_targets(&args(&["--target"])).is_err());
        assert!(parse_targets(&args(&["--bogus", "x"])).is_err());
    }

    #[test]
    fn displays_bytes_and_kib() {
        assert_eq!(Bytes(1025).to_string(), "1025 B (2 KiB)");
    }
}
