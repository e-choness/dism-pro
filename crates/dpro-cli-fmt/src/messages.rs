//! Message lookup from a Fluent (`.ftl`) catalog.
//!
//! Interim loader until the i18n pipeline (roadmap 3.4) brings the full Fluent runtime: it
//! understands `id = value`, indented multi-line values, `# comments`, and `{ $name }`
//! placeables. Selectors and terms aren't supported yet (none are used by the CLI catalog).

use std::{collections::HashMap, fmt::Display};

const EN_US_CLI: &str = include_str!("../../../i18n/en-US/cli.ftl");

#[derive(Debug, Clone, Default)]
pub struct Messages {
    map: HashMap<String, String>,
}

impl Messages {
    /// The built-in en-US catalog.
    #[must_use]
    pub fn en_us() -> Self {
        Self::parse(EN_US_CLI)
    }

    /// Parses a catalog. Lines it doesn't understand are skipped (`xtask i18n-check` rejects
    /// malformed catalogs before they ship).
    #[must_use]
    pub fn parse(src: &str) -> Self {
        let mut map = HashMap::new();
        let mut current: Option<(String, Vec<String>)> = None;
        for line in src.lines() {
            let indented = line.starts_with([' ', '\t']);
            if indented || (line.trim().is_empty() && current.is_some()) {
                if let Some((_, lines)) = current.as_mut() {
                    lines.push(line.to_owned());
                }
                continue;
            }
            if let Some((id, lines)) = current.take() {
                map.insert(id, join(&lines));
            }
            if line.starts_with('#') {
                continue;
            }
            if let Some((id, value)) = line.split_once('=') {
                current = Some((id.trim().to_owned(), vec![value.trim_start().to_owned()]));
            }
        }
        if let Some((id, lines)) = current {
            map.insert(id, join(&lines));
        }
        Self { map }
    }

    /// The message `id` with `{ $name }` placeables filled from `args`. A missing id renders
    /// as the id itself, so gaps are visible instead of silent.
    #[must_use]
    pub fn get(&self, id: &str, args: &[(&str, &dyn Display)]) -> String {
        let Some(template) = self.map.get(id) else {
            return id.to_owned();
        };
        let mut out = template.clone();
        for (name, value) in args {
            out = out.replace(&format!("{{ ${name} }}"), &value.to_string());
        }
        out
    }

    /// Shorthand for messages without arguments.
    #[must_use]
    pub fn text(&self, id: &str) -> String {
        self.get(id, &[])
    }
}

/// First line + continuation lines with their common indentation removed (Fluent rule).
fn join(lines: &[String]) -> String {
    let (first, rest) = lines.split_first().map_or(("", &[][..]), |(f, r)| (f.as_str(), r));
    let indent = rest
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    let mut parts: Vec<&str> = Vec::new();
    if !first.is_empty() {
        parts.push(first);
    }
    parts.extend(rest.iter().map(|l| l.get(indent..).unwrap_or("")));
    while parts.last().is_some_and(|l| l.trim().is_empty()) {
        parts.pop();
    }
    parts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_multiline_and_placeables() {
        let m = Messages::parse("# c\na = Hello { $who }!\nb =\n    line one\n      indented\n\nc = x\n");
        assert_eq!(m.get("a", &[("who", &"you")]), "Hello you!");
        assert_eq!(m.text("b"), "line one\n  indented");
        assert_eq!(m.text("c"), "x");
        assert_eq!(m.text("missing"), "missing");
    }

    #[test]
    fn built_in_catalog_has_every_change_kind() {
        let m = Messages::en_us();
        for kind in ["file_quarantine", "reg_set", "feature_state", "package_remove", "file_restore"] {
            let id = format!("change-{kind}");
            assert_ne!(m.text(&id), id, "{id} missing from cli.ftl");
        }
        assert!(m.text("cli-help").contains("targets list"));
    }
}
