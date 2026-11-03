//! The rows agents page through (TD-0013 §5.6): each list's fields, the few a default page shows
//! (so 50 rows stay within the 2,000-token budget), and what `filter` and `sort` read.

use dpro_core::plan::{Change, Reversibility};
use dpro_mcp::shape::{Row, SortKey};
use serde::Serialize;
use serde_json::Value;

use crate::json::{FindingJson, JournalJson, PackageJson};

impl Row for PackageJson {
    const FIELDS: &'static [&'static str] = &["identity", "state", "release_type", "installed"];
    const DEFAULT_FIELDS: &'static [&'static str] = &["identity", "state", "installed"];
    const SORTABLE: &'static [&'static str] = &["identity", "installed", "state"];

    fn filter_text(&self) -> &str {
        &self.identity
    }

    fn sort_key(&self, field: usize) -> SortKey<'_> {
        match field {
            0 => SortKey::Text(&self.identity),
            1 => SortKey::Text(&self.installed),
            _ => SortKey::Text(self.state),
        }
    }
}

impl Row for JournalJson {
    const FIELDS: &'static [&'static str] = &[
        "job",
        "op",
        "created",
        "status",
        "applied",
        "failed",
        "reboot_required",
        "undoes",
        "undone_by",
        "journal",
    ];
    const DEFAULT_FIELDS: &'static [&'static str] =
        &["job", "op", "created", "status", "undone_by"];
    const SORTABLE: &'static [&'static str] = &["created", "op", "status"];

    fn filter_text(&self) -> &str {
        &self.op
    }

    fn sort_key(&self, field: usize) -> SortKey<'_> {
        match field {
            0 => SortKey::Text(&self.created),
            1 => SortKey::Text(&self.op),
            _ => SortKey::Text(self.status),
        }
    }
}

impl Row for FindingJson {
    const FIELDS: &'static [&'static str] = &[
        "severity",
        "diagnosis",
        "message",
        "code",
        "log",
        "first_line",
        "last_line",
    ];
    const DEFAULT_FIELDS: &'static [&'static str] =
        &["severity", "diagnosis", "message", "code", "first_line"];
    const SORTABLE: &'static [&'static str] = &["first_line", "diagnosis"];

    fn filter_text(&self) -> &str {
        &self.message
    }

    fn sort_key(&self, field: usize) -> SortKey<'_> {
        match field {
            0 => SortKey::Number(self.first_line),
            _ => SortKey::Text(self.diagnosis),
        }
    }
}

/// One change of a stored plan, as `plan_get` and the `*_plan` tools show it.
#[derive(Debug, Serialize)]
pub(crate) struct ChangeRow {
    pub(crate) index: u64,
    pub(crate) kind: &'static str,
    /// The change itself, in the plan document's JSON.
    pub(crate) change: Value,
    /// `reversible`, `reversible_until`, `reversible_via` or `irreversible`.
    pub(crate) reversibility: &'static str,
    pub(crate) reboot_required: bool,
    /// What `filter` searches: the change's JSON as text.
    #[serde(skip)]
    text: String,
}

impl Row for ChangeRow {
    const FIELDS: &'static [&'static str] = &[
        "index",
        "kind",
        "change",
        "reversibility",
        "reboot_required",
    ];
    const DEFAULT_FIELDS: &'static [&'static str] = Self::FIELDS;
    const SORTABLE: &'static [&'static str] = &["index", "kind"];

    fn filter_text(&self) -> &str {
        &self.text
    }

    fn sort_key(&self, field: usize) -> SortKey<'_> {
        match field {
            0 => SortKey::Number(self.index),
            _ => SortKey::Text(self.kind),
        }
    }
}

/// The rows of a plan's changes, numbered from 0 as journals number them.
pub(crate) fn change_rows(changes: &[Change<String>]) -> Vec<ChangeRow> {
    (0_u64..)
        .zip(changes)
        .map(|(index, c)| {
            let change = serde_json::to_value(&c.kind).unwrap_or(Value::Null);
            ChangeRow {
                index,
                kind: c.kind.name(),
                text: change.to_string(),
                change,
                reversibility: match c.reversibility {
                    Reversibility::Reversible => "reversible",
                    Reversibility::ReversibleUntil { .. } => "reversible_until",
                    Reversibility::ReversibleVia { .. } => "reversible_via",
                    Reversibility::Irreversible { .. } => "irreversible",
                },
                reboot_required: c.reboot_required,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use dpro_core::{
        health::{LogFile, Severity},
        plan::ChangeKind,
    };

    use super::*;

    /// The field lists must match what the row serializes to, or `fields` would name columns
    /// that never appear (or hide ones that do).
    fn check<R: Row>(row: &R) {
        let Value::Object(map) = serde_json::to_value(row).unwrap() else {
            panic!("a row must serialize to an object");
        };
        let keys: BTreeSet<&str> = map.keys().map(String::as_str).collect();
        let fields: BTreeSet<&str> = R::FIELDS.iter().copied().collect();
        assert_eq!(keys, fields);
        for f in R::DEFAULT_FIELDS.iter().chain(R::SORTABLE) {
            assert!(fields.contains(f), "{f}");
        }
    }

    #[test]
    fn field_lists_match_the_rows() {
        check(&PackageJson {
            identity: "Package_for_KB1~31bf3856ad364e35~amd64~~1.0".into(),
            state: "installed",
            release_type: 7,
            installed: "2026-10-01T12:00:00".into(),
        });
        check(&JournalJson {
            job: "01J".into(),
            op: "features.enable".into(),
            created: "2026-10-01T12:00:00Z".into(),
            status: "completed",
            applied: 1,
            failed: 0,
            reboot_required: false,
            undoes: None,
            undone_by: None,
            journal: "C:\\ProgramData\\DismPro\\journal\\01J.dpj".into(),
        });
        check(&FindingJson {
            severity: Severity::Error,
            diagnosis: "health-finding-source-missing",
            message: "Repair files are missing.".into(),
            code: Some("0x800f081f".into()),
            log: LogFile::Cbs,
            first_line: 10,
            last_line: 12,
        });
        let rows = change_rows(&[Change {
            kind: ChangeKind::FeatureState {
                name: "SMB1Protocol".into(),
                enable: false,
                source: None,
            },
            reversibility: Reversibility::Reversible,
            reboot_required: true,
        }]);
        check(&rows[0]);
        assert_eq!(
            rows[0].kind,
            ChangeKind::<String>::FeatureState {
                name: String::new(),
                enable: false,
                source: None,
            }
            .name()
        );
        assert!(rows[0].filter_text().contains("SMB1Protocol"));
    }
}
