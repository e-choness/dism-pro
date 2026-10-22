//! `cargo xtask layering`: enforces doc 02 §2 on every workspace member.
//!
//! 1. Workspace dependencies only point down the layer diagram (`RULES` below).
//! 2. `pure` crates have no OS dependencies at all.
//! 3. Every crate except the `ffi` ones has `#![forbid(unsafe_code)]` in its root.
//!
//! Changing the table means changing doc 02 §2 in the same PR.

use std::{fs, path::Path};

use crate::{Result, manifest};

/// crate, kind, allowed workspace dependencies (`-` none, `*` any except `dpro-sys`).
/// Kinds: `pure` = OS-free, `ffi` = may use `unsafe`, `lib`, `bin`, `tool`.
const RULES: &str = "
dpro-core     pure  -
dpro-rules    pure  dpro-core
dpro-sys      ffi   -
dpro-win      ffi   dpro-core,dpro-sys
dpro-broker   lib   dpro-core,dpro-win
dpro-engine   lib   dpro-core,dpro-rules,dpro-win
dpro-app      lib   dpro-core,dpro-rules,dpro-engine
dpro-ui       lib   dpro-core,dpro-app,dpro-win
dpro-cli-fmt  lib   dpro-core,dpro-app
dpro-mcp      lib   dpro-core,dpro-rules,dpro-app,dpro-broker
dismpro       bin   *
dismpro-cli   bin   *
xtask         tool  -
";

const FORBID_UNSAFE: &str = "#![forbid(unsafe_code)]";

#[derive(Debug)]
struct Rule<'a> {
    krate: &'a str,
    kind: &'a str,
    /// `None` = any workspace crate except `dpro-sys`.
    allowed: Option<Vec<&'a str>>,
}

fn rules() -> Vec<Rule<'static>> {
    RULES
        .lines()
        .filter_map(|line| {
            let mut cols = line.split_whitespace();
            let krate = cols.next()?;
            let kind = cols.next()?;
            let allowed = match cols.next()? {
                "*" => None,
                "-" => Some(Vec::new()),
                list => Some(list.split(',').collect()),
            };
            Some(Rule { krate, kind, allowed })
        })
        .collect()
}

/// One workspace member as the check sees it.
#[derive(Debug)]
pub(crate) struct Member {
    pub(crate) manifest: manifest::Manifest,
    /// Contents of `src/lib.rs` or `src/main.rs`, if present.
    pub(crate) root_source: Option<String>,
}

pub(crate) fn run(root: &Path) -> Result<()> {
    let members = load_members(root)?;
    let violations = check(&members);
    if violations.is_empty() {
        println!("layering: {} crates OK", members.len());
        return Ok(());
    }
    for v in &violations {
        println!("layering: {v}");
    }
    Err(format!("{} layering violation(s), see doc 02 §2", violations.len()).into())
}

fn load_members(root: &Path) -> Result<Vec<Member>> {
    let mut dirs = vec![root.join("xtask")];
    for entry in fs::read_dir(root.join("crates"))? {
        let path = entry?.path();
        if path.join("Cargo.toml").is_file() {
            dirs.push(path);
        }
    }
    dirs.sort();

    let mut members = Vec::with_capacity(dirs.len());
    for dir in dirs {
        let toml_path = dir.join("Cargo.toml");
        let text = fs::read_to_string(&toml_path)?;
        let parsed = manifest::parse(&text);
        let manifest = parsed.map_err(|e| format!("{}: {e}", toml_path.display()))?;
        let root_source = ["src/lib.rs", "src/main.rs"]
            .iter()
            .find_map(|f| fs::read_to_string(dir.join(f)).ok());
        members.push(Member {
            manifest,
            root_source,
        });
    }
    Ok(members)
}

pub(crate) fn check(members: &[Member]) -> Vec<String> {
    let rules = rules();
    let names: Vec<&str> = members.iter().map(|m| m.manifest.name.as_str()).collect();
    let mut out = Vec::new();

    for member in members {
        let name = member.manifest.name.as_str();
        let Some(rule) = rules.iter().find(|r| r.krate == name) else {
            out.push(format!("{name}: no rule; add one to xtask/src/layering.rs"));
            continue;
        };

        for dep in &member.manifest.deps {
            let dep = dep.as_str();
            let allowed = match &rule.allowed {
                Some(list) => list.contains(&dep),
                None => dep != "dpro-sys",
            };
            if names.contains(&dep) && !allowed {
                out.push(format!("{name} must not depend on {dep}"));
            }
            if rule.kind == "pure" && is_os_crate(dep) {
                out.push(format!("{name} is OS-free and must not depend on {dep}"));
            }
        }

        let source = member.root_source.as_deref().unwrap_or_default();
        let forbids_unsafe = source.lines().any(|l| l.trim() == FORBID_UNSAFE);
        if rule.kind != "ffi" && !forbids_unsafe {
            out.push(format!("{name}: crate root needs {FORBID_UNSAFE}"));
        }
    }
    out
}

fn is_os_crate(dep: &str) -> bool {
    matches!(dep, "windows" | "winapi" | "dpro-sys" | "dpro-win") || dep.starts_with("windows-")
}

#[cfg(test)]
mod tests {
    use super::*;

    const KINDS: [&str; 5] = ["pure", "ffi", "lib", "bin", "tool"];
    const SAFE: &str = "//! x\n\n#![forbid(unsafe_code)]\n";

    fn member(name: &str, deps: &[&str], source: &str) -> Member {
        Member {
            manifest: manifest::Manifest {
                name: name.to_owned(),
                deps: deps.iter().map(|d| (*d).to_owned()).collect(),
            },
            root_source: Some(source.to_owned()),
        }
    }

    #[test]
    fn rule_table_parses() {
        let rules = rules();
        assert_eq!(rules.len(), 13);
        assert!(rules.iter().all(|r| KINDS.contains(&r.kind)));
    }

    #[test]
    fn clean_graph_passes() {
        let ms = [
            member("dpro-core", &[], SAFE),
            member("dpro-sys", &[], ""),
            member("dpro-win", &["dpro-core", "dpro-sys", "windows-sys"], ""),
            member("dpro-engine", &["dpro-core", "dpro-win"], SAFE),
            member("dismpro-cli", &["dpro-engine", "dpro-win"], SAFE),
        ];
        assert!(check(&ms).is_empty());
    }

    #[test]
    fn upward_and_ffi_edges_fail() {
        let ms = [
            member("dpro-core", &[], SAFE),
            member("dpro-sys", &[], ""),
            member("dpro-engine", &["dpro-core", "dpro-sys"], SAFE),
            member("dismpro", &["dpro-sys"], SAFE),
            member("dpro-rules", &["dpro-engine"], SAFE),
        ];
        let expected = [
            "dpro-engine must not depend on dpro-sys",
            "dismpro must not depend on dpro-sys",
            "dpro-rules must not depend on dpro-engine",
        ];
        assert_eq!(check(&ms), expected);
    }

    #[test]
    fn pure_crates_reject_os_crates() {
        let ms = [member("dpro-core", &["windows-sys", "serde"], SAFE)];
        let expected = ["dpro-core is OS-free and must not depend on windows-sys"];
        assert_eq!(check(&ms), expected);
    }

    #[test]
    fn unsafe_must_be_forbidden_outside_ffi_crates() {
        let ms = [member("dpro-app", &[], "//! x\n"), member("dpro-win", &[], "")];
        let expected = ["dpro-app: crate root needs #![forbid(unsafe_code)]"];
        assert_eq!(check(&ms), expected);
    }

    #[test]
    fn unknown_crates_need_a_rule() {
        let ms = [member("dpro-new", &[], SAFE)];
        assert_eq!(check(&ms).len(), 1);
    }
}
