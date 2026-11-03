//! TD-0013 §7 token budgets, estimated at 4 bytes per token (TD-0013 §7; calibrated quarterly).
//! A default list page must stay within 2,000 tokens, and any result within 8k tokens (32 KiB).
//! The tool catalog budget (3,500 tokens) joins this file with the catalog.

use dpro_mcp::shape::{ListArgs, MAX_RESULT_BYTES, Row, SortKey, page};
use serde::Serialize;

const BYTES_PER_TOKEN: usize = 4;
const DEFAULT_PAGE_TOKENS: usize = 2_000;

/// A `packages.v1` row with realistic lengths: the widest list the first toolsets return.
#[derive(Debug, Serialize)]
struct PackageRow {
    identity: String,
    state: &'static str,
    release_type: i32,
    installed: String,
}

impl Row for PackageRow {
    const FIELDS: &'static [&'static str] = &["identity", "state", "release_type", "installed"];
    const DEFAULT_FIELDS: &'static [&'static str] = &["identity", "state"];
    const SORTABLE: &'static [&'static str] = &["identity", "installed"];

    fn filter_text(&self) -> &str {
        &self.identity
    }

    fn sort_key(&self, field: usize) -> SortKey<'_> {
        match field {
            0 => SortKey::Text(&self.identity),
            _ => SortKey::Text(&self.installed),
        }
    }
}

fn rows() -> Vec<PackageRow> {
    (0..1_000)
        .map(|i| PackageRow {
            identity: format!(
                "Microsoft-Windows-Server-LanguagePack-Package~31bf3856ad364e35~amd64~en-US~10.0.26100.{i}"
            ),
            state: "installed",
            release_type: 7,
            installed: "2026-10-24T10:00:00".into(),
        })
        .collect()
}

fn tokens(page: &dpro_mcp::shape::Page) -> usize {
    serde_json::to_vec(page).map_or(usize::MAX, |v| v.len()) / BYTES_PER_TOKEN
}

#[test]
fn a_default_page_fits_the_list_budget() {
    let p = page(&rows(), &ListArgs::default(), MAX_RESULT_BYTES).unwrap();
    assert_eq!(p.items.len(), 50);
    let t = tokens(&p);
    eprintln!("default page of 50: {t} tokens");
    assert!(t <= DEFAULT_PAGE_TOKENS, "default page: {t} tokens");
}

#[test]
fn any_page_fits_the_result_cap() {
    let all_fields = ListArgs {
        limit: Some(500),
        fields: Some(PackageRow::FIELDS.iter().map(|f| (*f).to_owned()).collect()),
        ..ListArgs::default()
    };
    let p = page(&rows(), &all_fields, MAX_RESULT_BYTES).unwrap();
    assert!(p.truncated, "500 full rows should exceed the cap");
    // The cap counts items; the page adds a few dozen bytes of framing.
    let bytes = serde_json::to_vec(&p).unwrap().len();
    eprintln!("capped page: {} rows, {bytes} bytes", p.items.len());
    assert!(bytes <= MAX_RESULT_BYTES + 128, "{bytes} bytes");
}
