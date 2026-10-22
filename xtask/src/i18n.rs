//! `cargo xtask i18n-check [--release]`: consistency of `i18n/<locale>/*.ftl` (ADR-0005).
//!
//! Always fails on: an unparsable entry line, a duplicate message ID within a locale, or an ID
//! that a locale has but `en-US` (the source locale) doesn't. Tier-1 locales missing IDs are
//! warnings, and errors with `--release` (tier 1 must be 100 % before release).
//! Generating the Rust ID enum from `en-US` lands with the i18n pipeline (roadmap 3.4).

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use crate::Result;

const SOURCE: &str = "en-US";
const TIER1: [&str; 6] = ["en-US", "zh-Hans", "zh-Hant", "ja", "ru", "de"];

/// Message and term IDs per locale, with the file each came from.
type Catalogs = BTreeMap<String, Vec<(String, String)>>;

#[derive(Debug, Default, PartialEq, Eq)]
struct Report {
    errors: Vec<String>,
    warnings: Vec<String>,
}

pub(crate) fn run(root: &Path, args: &[String]) -> Result<()> {
    let release = match args {
        [] => false,
        [flag] if flag == "--release" => true,
        _ => return Err("i18n-check: only --release is accepted".into()),
    };
    let dir = root.join("i18n");
    if !dir.is_dir() {
        println!("i18n-check: no i18n/ catalogs yet");
        return Ok(());
    }

    let mut catalogs = Catalogs::new();
    let mut report = Report::default();
    for locale_dir in sorted_entries(&dir)? {
        if !locale_dir.is_dir() {
            continue;
        }
        let locale = file_name(&locale_dir);
        let ids = catalogs.entry(locale.clone()).or_default();
        for file in sorted_entries(&locale_dir)? {
            if file.extension().is_none_or(|e| e != "ftl") {
                continue;
            }
            let label = format!("{locale}/{}", file_name(&file));
            match parse_ids(&fs::read_to_string(&file)?) {
                Ok(found) => ids.extend(found.into_iter().map(|id| (id, label.clone()))),
                Err(e) => report.errors.push(format!("{label}: {e}")),
            }
        }
    }

    let checked = check(&catalogs, release);
    report.errors.extend(checked.errors);
    report.warnings.extend(checked.warnings);
    for w in &report.warnings {
        println!("i18n-check: warning: {w}");
    }
    for e in &report.errors {
        println!("i18n-check: error: {e}");
    }
    if !report.errors.is_empty() {
        return Err(format!("{} i18n error(s)", report.errors.len()).into());
    }
    println!("i18n-check: {} locale(s) OK", catalogs.len());
    Ok(())
}

fn sorted_entries(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir)? {
        out.push(entry?.path());
    }
    out.sort();
    Ok(out)
}

fn file_name(path: &Path) -> String {
    path.file_name().unwrap_or_default().to_string_lossy().into_owned()
}

/// IDs of the messages (`id = …`) and terms (`-id = …`) in one `.ftl` file, in order.
/// Fluent requires continuation lines to be indented, so every non-blank line that starts
/// in column 0 is an entry, a comment, or the closing `}` of a multi-line placeable.
fn parse_ids(text: &str) -> std::result::Result<Vec<String>, String> {
    let mut ids = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let starts_entry = line.starts_with(|c: char| !c.is_whitespace());
        if !starts_entry || line.starts_with('#') || line.starts_with('}') {
            continue;
        }
        let id = line.split_once('=').map(|(id, _)| id.trim_end());
        match id {
            Some(id) if is_identifier(id.strip_prefix('-').unwrap_or(id)) => {
                ids.push(id.to_owned());
            }
            _ => return Err(format!("line {}: expected `id = value`", n + 1)),
        }
    }
    Ok(ids)
}

/// Fluent identifier: `[a-zA-Z][a-zA-Z0-9_-]*`.
fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn check(catalogs: &Catalogs, release: bool) -> Report {
    let mut report = Report::default();
    let Some(source) = catalogs.get(SOURCE) else {
        if !catalogs.is_empty() {
            report.errors.push(format!("source locale {SOURCE} is missing"));
        }
        return report;
    };
    let source_ids: BTreeSet<&str> = source.iter().map(|(id, _)| id.as_str()).collect();

    for (locale, ids) in catalogs {
        let mut seen = BTreeSet::new();
        for (id, file) in ids {
            if !seen.insert(id.as_str()) {
                report.errors.push(format!("{file}: duplicate ID {id}"));
            }
            if !source_ids.contains(id.as_str()) {
                report.errors.push(format!("{file}: {id} is not in {SOURCE}"));
            }
        }
        let missing = source_ids.difference(&seen).count();
        if missing > 0 && TIER1.contains(&locale.as_str()) {
            let msg = format!("{locale}: {missing} ID(s) missing (tier-1 locale)");
            if release {
                report.errors.push(msg);
            } else {
                report.warnings.push(msg);
            }
        }
    }

    for locale in TIER1 {
        if !catalogs.contains_key(locale) {
            let msg = format!("{locale}: tier-1 locale has no catalog");
            if release {
                report.errors.push(msg);
            } else {
                report.warnings.push(msg);
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalogs(entries: &[(&str, &[&str])]) -> Catalogs {
        entries
            .iter()
            .map(|(locale, ids)| {
                let file = format!("{locale}/main.ftl");
                let ids: Vec<(String, String)> =
                    ids.iter().map(|id| ((*id).to_owned(), file.clone())).collect();
                ((*locale).to_owned(), ids)
            })
            .collect()
    }

    #[test]
    fn parses_messages_terms_and_skips_continuations() {
        let ftl = "\
# comment
-brand = DISM Pro
nav-clean = Clean
    .tooltip = Free up space
apply-count = { $count ->
    [one] Apply one change
   *[other] Apply { $count } changes
}

review-changes=Review changes
";
        let ids = parse_ids(ftl).unwrap();
        assert_eq!(ids, ["-brand", "nav-clean", "apply-count", "review-changes"]);
    }

    #[test]
    fn rejects_unindented_junk() {
        assert!(parse_ids("ok = fine\nnot an entry\n").is_err());
        assert!(parse_ids("9lives = no\n").is_err());
    }

    #[test]
    fn duplicates_and_orphans_are_errors() {
        let c = catalogs(&[("en-US", &["a", "b", "a"]), ("de", &["a", "c"])]);
        let r = check(&c, false);
        let expected = [
            "de/main.ftl: c is not in en-US",
            "en-US/main.ftl: duplicate ID a",
        ];
        assert_eq!(r.errors, expected);
    }

    #[test]
    fn missing_tier1_ids_fail_only_for_release() {
        let c = catalogs(&[("en-US", &["a", "b"]), ("ja", &["a"]), ("fr", &[])]);
        let dev = check(&c, false);
        assert!(dev.errors.is_empty());
        assert!(dev.warnings.contains(&"ja: 1 ID(s) missing (tier-1 locale)".to_owned()));
        let release = check(&c, true);
        assert!(release.errors.contains(&"ja: 1 ID(s) missing (tier-1 locale)".to_owned()));
        assert!(release.errors.contains(&"ru: tier-1 locale has no catalog".to_owned()));
        assert!(!release.errors.iter().any(|e| e.starts_with("fr")));
    }

    #[test]
    fn empty_tree_passes_and_missing_source_fails() {
        assert_eq!(check(&Catalogs::new(), false), Report::default());
        let c = catalogs(&[("de", &["a"])]);
        assert_eq!(check(&c, false).errors, ["source locale en-US is missing"]);
    }
}
