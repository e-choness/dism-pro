//! Response shaping for list tools (TD-0013 §5.6). A list comes back one page at a time: at most
//! `limit` rows (default 50), narrowed by `filter`, ordered by `sort`, projected to `fields`. The
//! page is cut short at a byte cap, with `truncated: true` and a cursor for the rest.
//!
//! Cursors are self-contained (an offset plus a fingerprint of the query), because a 2026-07-28
//! server keeps no state between requests (TD-0013 §15). A cursor from another query, or from a
//! list that has since grown or shrunk, is refused rather than silently misread.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const DEFAULT_LIMIT: u32 = 50;
pub const MAX_LIMIT: u32 = 500;
/// TD-0013 §5.6: one tool result stays under 8k tokens, about 32 KiB.
pub const MAX_RESULT_BYTES: usize = 32 * 1024;

/// A value rows are ordered by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SortKey<'a> {
    Number(u64),
    Text(&'a str),
}

/// A row of a list tool.
pub trait Row: Serialize {
    /// Every field a row serializes to, as `fields` may name them.
    const FIELDS: &'static [&'static str];
    /// The fields of a page when the call names none: few enough that a default page stays
    /// within the 2,000-token list budget (TD-0013 §7).
    const DEFAULT_FIELDS: &'static [&'static str];
    /// The fields `sort` accepts.
    const SORTABLE: &'static [&'static str];

    /// The text `filter` searches: an ASCII case-insensitive substring match.
    fn filter_text(&self) -> &str;

    /// The value of `SORTABLE[field]`.
    fn sort_key(&self, field: usize) -> SortKey<'_>;
}

/// The list arguments every list tool takes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ListArgs {
    pub limit: Option<u32>,
    pub cursor: Option<String>,
    /// Defaults to [`Row::DEFAULT_FIELDS`].
    pub fields: Option<Vec<String>>,
    pub filter: Option<String>,
    /// A sortable field; a leading `-` sorts descending.
    pub sort: Option<String>,
}

/// One page of a list.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Page {
    pub items: Vec<Map<String, Value>>,
    /// Rows that match the filter, over all pages.
    pub matched: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    /// The byte cap ended this page before `limit` rows.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ShapeError {
    #[error("limit must be between 1 and {MAX_LIMIT}")]
    Limit,
    #[error("unknown field: {0}")]
    Field(String),
    #[error("can't sort by: {0}")]
    Sort(String),
    #[error("the cursor belongs to another query or an older list; list again without it")]
    Cursor,
    #[error("a row didn't serialize to an object")]
    Row,
}

/// Shapes `rows` into the page `args` asks for, at most `max_bytes` of items (at least one row,
/// so a caller always makes progress).
///
/// # Errors
/// Invalid arguments ([`ShapeError`]); the agent gets the message and can correct its call.
pub fn page<R: Row>(rows: &[R], args: &ListArgs, max_bytes: usize) -> Result<Page, ShapeError> {
    let limit = args.limit.unwrap_or(DEFAULT_LIMIT);
    if limit == 0 || limit > MAX_LIMIT {
        return Err(ShapeError::Limit);
    }
    let limit = usize::try_from(limit).map_err(|_| ShapeError::Limit)?;
    let fields: Vec<&str> = match &args.fields {
        Some(names) => names.iter().map(String::as_str).collect(),
        None => R::DEFAULT_FIELDS.to_vec(),
    };
    if let Some(f) = fields.iter().find(|f| !R::FIELDS.contains(f)) {
        return Err(ShapeError::Field((*f).to_owned()));
    }
    let sort = args.sort.as_deref().map(parse_sort::<R>).transpose()?;
    let filter = args.filter.as_deref().filter(|f| !f.is_empty());

    // Without filter or sort the page is a slice: no pass over the other rows.
    let order: Option<Vec<usize>> = if filter.is_none() && sort.is_none() {
        None
    } else {
        let mut order: Vec<usize> = match filter {
            Some(needle) => (0..rows.len())
                .filter(|&i| contains_ignore_ascii_case(rows[i].filter_text(), needle))
                .collect(),
            None => (0..rows.len()).collect(),
        };
        if let Some((field, descending)) = sort {
            // Stable, so equal keys keep the source order and pages don't overlap.
            order.sort_by(|&a, &b| {
                let o = rows[a].sort_key(field).cmp(&rows[b].sort_key(field));
                if descending { o.reverse() } else { o }
            });
        }
        Some(order)
    };
    let matched = order.as_ref().map_or(rows.len(), Vec::len);
    let fingerprint = fingerprint(filter, args.sort.as_deref(), matched);
    let start = match args.cursor.as_deref() {
        Some(c) => parse_cursor(c, fingerprint, matched)?,
        None => 0,
    };

    let mut items = Vec::with_capacity(limit.min(matched - start));
    let mut bytes = 0;
    let mut truncated = false;
    for pos in start..matched.min(start + limit) {
        let row = &rows[order.as_ref().map_or(pos, |o| o[pos])];
        let Ok(Value::Object(mut item)) = serde_json::to_value(row) else {
            return Err(ShapeError::Row);
        };
        if !fields.is_empty() {
            item.retain(|k, _| fields.contains(&k.as_str()));
        }
        // The serialized size plus a separating comma.
        let size = serde_json::to_vec(&item).map_or(0, |v| v.len()) + 1;
        if bytes + size > max_bytes && !items.is_empty() {
            truncated = true;
            break;
        }
        bytes += size;
        items.push(item);
    }
    let end = start + items.len();
    let next_cursor = (end < matched).then(|| format!("{end}.{fingerprint:016x}"));
    Ok(Page {
        items,
        matched,
        next_cursor,
        truncated,
    })
}

fn parse_sort<R: Row>(sort: &str) -> Result<(usize, bool), ShapeError> {
    let (name, descending) = match sort.strip_prefix('-') {
        Some(name) => (name, true),
        None => (sort, false),
    };
    R::SORTABLE
        .iter()
        .position(|&f| f == name)
        .map(|i| (i, descending))
        .ok_or_else(|| ShapeError::Sort(sort.to_owned()))
}

fn parse_cursor(cursor: &str, fingerprint: u64, matched: usize) -> Result<usize, ShapeError> {
    let (offset, fp) = cursor.split_once('.').ok_or(ShapeError::Cursor)?;
    let offset: usize = offset.parse().map_err(|_| ShapeError::Cursor)?;
    let fp = u64::from_str_radix(fp, 16).map_err(|_| ShapeError::Cursor)?;
    if fp != fingerprint || offset > matched {
        return Err(ShapeError::Cursor);
    }
    Ok(offset)
}

/// FNV-1a over what decides the order and length of the matched rows. Not a security boundary:
/// a forged cursor only selects another page of a list the caller may read anyway.
fn fingerprint(filter: Option<&str>, sort: Option<&str>, matched: usize) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: &[u8]| {
        for &b in bytes {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    eat(filter.unwrap_or_default().as_bytes());
    eat(&[0]);
    eat(sort.unwrap_or_default().as_bytes());
    eat(&[0]);
    eat(&u64::try_from(matched).unwrap_or(u64::MAX).to_le_bytes());
    h
}

/// ASCII case-insensitive substring search; other bytes match exactly.
fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    let (h, n) = (haystack.as_bytes(), needle.as_bytes());
    n.len() <= h.len() && h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Serialize)]
    struct Item {
        name: String,
        size: u64,
        state: &'static str,
    }

    impl Row for Item {
        const FIELDS: &'static [&'static str] = &["name", "size", "state"];
        const DEFAULT_FIELDS: &'static [&'static str] = &["name", "size"];
        const SORTABLE: &'static [&'static str] = &["name", "size"];

        fn filter_text(&self) -> &str {
            &self.name
        }

        fn sort_key(&self, field: usize) -> SortKey<'_> {
            match field {
                0 => SortKey::Text(&self.name),
                _ => SortKey::Number(self.size),
            }
        }
    }

    fn items(n: u64) -> Vec<Item> {
        (0..n)
            .map(|i| Item {
                name: format!("{}-item-{i:03}", if i % 2 == 0 { "Even" } else { "odd" }),
                size: (i * 7) % 10,
                state: "installed",
            })
            .collect()
    }

    fn names(p: &Page) -> Vec<&str> {
        p.items
            .iter()
            .map(|i| i["name"].as_str().unwrap_or_default())
            .collect()
    }

    fn args(limit: u32) -> ListArgs {
        ListArgs {
            limit: Some(limit),
            ..ListArgs::default()
        }
    }

    #[test]
    fn pages_through_every_row_once() {
        let rows = items(23);
        let mut a = args(5);
        let mut seen = Vec::new();
        loop {
            let p = page(&rows, &a, MAX_RESULT_BYTES).unwrap();
            assert_eq!(p.matched, 23);
            seen.extend(names(&p).into_iter().map(str::to_owned));
            match p.next_cursor {
                Some(c) => a.cursor = Some(c),
                None => break,
            }
        }
        let all: Vec<String> = rows.iter().map(|r| r.name.clone()).collect();
        assert_eq!(seen, all);
    }

    #[test]
    fn filters_case_insensitively_and_sorts_stably() {
        let rows = items(10);
        let a = ListArgs {
            filter: Some("EVEN".into()),
            sort: Some("-size".into()),
            ..args(50)
        };
        let p = page(&rows, &a, MAX_RESULT_BYTES).unwrap();
        assert_eq!(p.matched, 5);
        let sizes: Vec<u64> = p.items.iter().filter_map(|i| i["size"].as_u64()).collect();
        assert_eq!(sizes, [8, 6, 4, 2, 0]);
        assert!(p.next_cursor.is_none());
    }

    #[test]
    fn projects_the_default_or_the_named_fields() {
        let rows = items(3);
        let default = page(&rows, &args(50), MAX_RESULT_BYTES).unwrap();
        assert!(
            default
                .items
                .iter()
                .all(|i| i.len() == 2 && !i.contains_key("state"))
        );
        let named = ListArgs {
            fields: Some(vec!["state".into()]),
            ..args(50)
        };
        let p = page(&rows, &named, MAX_RESULT_BYTES).unwrap();
        assert!(
            p.items
                .iter()
                .all(|i| i.len() == 1 && i.contains_key("state"))
        );
    }

    #[test]
    fn the_byte_cap_truncates_but_always_returns_a_row() {
        let rows = items(10);
        let p = page(&rows, &args(10), 100).unwrap();
        assert!(p.truncated);
        assert!(!p.items.is_empty() && p.items.len() < 10);
        let next = p.next_cursor.clone().unwrap();
        let rest = page(
            &rows,
            &ListArgs {
                cursor: Some(next),
                ..args(10)
            },
            MAX_RESULT_BYTES,
        )
        .unwrap();
        assert_eq!(p.items.len() + rest.items.len(), 10);
        let tiny = page(&rows, &args(10), 1).unwrap();
        assert_eq!(tiny.items.len(), 1);
    }

    #[test]
    fn refuses_bad_arguments() {
        let rows = items(10);
        assert_eq!(page(&rows, &args(0), 1024), Err(ShapeError::Limit));
        assert_eq!(
            page(&rows, &args(MAX_LIMIT + 1), 1024),
            Err(ShapeError::Limit)
        );
        let bad_field = ListArgs {
            fields: Some(vec!["nope".into()]),
            ..args(5)
        };
        assert_eq!(
            page(&rows, &bad_field, 1024),
            Err(ShapeError::Field("nope".into()))
        );
        let bad_sort = ListArgs {
            sort: Some("state".into()),
            ..args(5)
        };
        assert_eq!(
            page(&rows, &bad_sort, 1024),
            Err(ShapeError::Sort("state".into()))
        );
    }

    #[test]
    fn a_cursor_only_works_for_its_own_query_and_list() {
        let rows = items(10);
        let first = page(&rows, &args(3), MAX_RESULT_BYTES).unwrap();
        let cursor = first.next_cursor.unwrap();
        let other_query = ListArgs {
            cursor: Some(cursor.clone()),
            filter: Some("odd".into()),
            ..args(3)
        };
        assert_eq!(page(&rows, &other_query, 1024), Err(ShapeError::Cursor));
        let grown = items(11);
        let same_query = ListArgs {
            cursor: Some(cursor),
            ..args(3)
        };
        assert_eq!(page(&grown, &same_query, 1024), Err(ShapeError::Cursor));
        for junk in ["", "x", "3", "3.zz", "99.0"] {
            let a = ListArgs {
                cursor: Some(junk.into()),
                ..args(3)
            };
            assert_eq!(page(&rows, &a, 1024), Err(ShapeError::Cursor), "{junk:?}");
        }
    }

    #[test]
    fn unknown_list_arguments_are_rejected() {
        let parsed: Result<ListArgs, _> = serde_json::from_str(r#"{"limit": 5, "offset": 2}"#);
        assert!(parsed.is_err());
    }
}
