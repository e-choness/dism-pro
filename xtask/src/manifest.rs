//! Minimal reader for the workspace's own `Cargo.toml` files. std only, so it understands
//! just the subset of TOML our manifests use: `[package] name`, and dependency keys in
//! `[dependencies]`, `[dev-dependencies]`, `[build-dependencies]`, their `target.'cfg(..)'.`
//! forms, and `[dependencies.<name>]` tables, with `package = "…"` renames.

use std::collections::BTreeMap;

/// What the layering check needs from one member manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Manifest {
    pub(crate) name: String,
    /// Real (un-renamed) crate names of every dependency, sorted and deduplicated.
    pub(crate) deps: Vec<String>,
}

#[derive(Debug)]
enum Section {
    Package,
    Deps,
    DepTable(String),
    Other,
}

pub(crate) fn parse(text: &str) -> Result<Manifest, String> {
    let mut section = Section::Other;
    let mut name = None;
    // alias (the key in the manifest) -> real crate name
    let mut deps: BTreeMap<String, String> = BTreeMap::new();

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            section = parse_header(line);
            if let Section::DepTable(alias) = &section {
                deps.entry(alias.clone()).or_insert_with(|| alias.clone());
            }
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue; // continuation of a multi-line array or inline table
        };
        let key = unquote(key.trim());
        let value = value.trim();
        match &section {
            Section::Package if key == "name" => name = Some(unquote(value)),
            Section::Deps => {
                let (alias, field) = key.split_once('.').unwrap_or((key.as_str(), ""));
                let alias = unquote(alias);
                let real = if field == "package" {
                    Some(unquote(value))
                } else {
                    inline_package(value)
                };
                let entry = deps.entry(alias.clone()).or_insert(alias);
                if let Some(real) = real {
                    *entry = real;
                }
            }
            Section::DepTable(alias) if key == "package" => {
                deps.insert(alias.clone(), unquote(value));
            }
            _ => {}
        }
    }

    let name = name.ok_or("missing [package] name")?;
    let mut deps: Vec<String> = deps.into_values().collect();
    deps.sort_unstable();
    deps.dedup();
    Ok(Manifest { name, deps })
}

fn parse_header(line: &str) -> Section {
    if line.starts_with("[[") {
        return Section::Other; // [[bin]], [[bench]], …
    }
    let header = line.trim_start_matches('[');
    let header = header.split(']').next().unwrap_or_default().trim();
    if header == "package" {
        return Section::Package;
    }
    if is_dep_table(header) {
        return Section::Deps;
    }
    if let Some((prefix, alias)) = header.rsplit_once('.')
        && is_dep_table(prefix)
    {
        return Section::DepTable(unquote(alias));
    }
    Section::Other
}

fn is_dep_table(header: &str) -> bool {
    ["dependencies", "dev-dependencies", "build-dependencies"]
        .iter()
        .any(|t| header == *t || header.ends_with(&format!(".{t}")))
}

/// `{ package = "real", … }` → `real`.
fn inline_package(value: &str) -> Option<String> {
    let body = value.strip_prefix('{')?;
    body.split(',').find_map(|field| {
        let (k, v) = field.split_once('=')?;
        (k.trim() == "package").then(|| unquote(v.trim().trim_end_matches('}').trim()))
    })
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    for q in ['"', '\''] {
        if let Some(inner) = s.strip_prefix(q)
            && let Some(end) = inner.find(q)
        {
            return inner[..end].to_owned();
        }
    }
    s.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_name_and_all_dependency_forms() {
        let m = parse(
            r#"
[package]
name = "dpro-engine"
version.workspace = true

[dependencies]
dpro-core.workspace = true
dpro-rules = { path = "../dpro-rules" }
renamed = { package = "real-crate", version = "1" }
other.package = "dotted-real"

[target.'cfg(windows)'.dependencies]
dpro-win = { workspace = true }

[dev-dependencies.proptest]
version = "1"

[build-dependencies.alias]
package = "build-real"

[features]
default = []

[[bench]]
name = "journal"
"#,
        )
        .unwrap();
        assert_eq!(m.name, "dpro-engine");
        assert_eq!(
            m.deps,
            [
                "build-real",
                "dotted-real",
                "dpro-core",
                "dpro-rules",
                "dpro-win",
                "proptest",
                "real-crate",
            ]
        );
    }

    #[test]
    fn multi_line_values_are_not_dependencies() {
        let m = parse(
            "[package]\nname = \"x\"\n[dependencies]\nfoo = { version = \"1\", features = [\n  \"a\",\n] }\n",
        )
        .unwrap();
        assert_eq!(m.deps, ["foo"]);
    }

    #[test]
    fn missing_name_is_an_error() {
        assert!(parse("[dependencies]\nfoo = \"1\"\n").is_err());
    }
}
