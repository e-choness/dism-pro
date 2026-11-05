//! Command palette ranking (F-10, TD-0009 §5.1 and §7 U10).
//!
//! The palette folds its entries once when it opens (or the language changes), then ranks every
//! entry on each keystroke: O(entries × query length), within 1 ms for 1,000 entries.
//!
//! A query is split into terms at whitespace, and every term must match the entry's label or its
//! keywords. From best to worst, a term can match the label:
//! 1. at the start of a word ("pack" in "Remove package"), best at the label's start;
//! 2. anywhere ("ckag");
//! 3. as word initials ("rp" in "Remove package");
//! 4. as a subsequence ("rmpk").
//!
//! Keywords count only for the first two kinds, at a third of the weight. A label that starts with
//! the whole query, recent use and a short label add smaller bonuses. Ties keep registry order.

use crate::fold::search_text;

/// The most bytes of a folded label, keywords or query that ranking looks at.
const TEXT_MAX: usize = 256;

const WORD_PREFIX: u32 = 900;
const AT_START: u32 = 100;
const SUBSTRING: u32 = 600;
const INITIALS: u32 = 400;
const SUBSEQUENCE: u32 = 200;
const KEYWORD_DIVISOR: u32 = 3;
const WHOLE_QUERY_PREFIX: u32 = 500;
/// Recency adds `RECENT_TOP - 3 × rank` for ranks below [`RECENT_RANKS`].
const RECENT_TOP: u32 = 64;
const RECENT_RANKS: u8 = 20;
/// An empty query lists recent entries first.
const EMPTY_RECENT: u32 = 1_000;

/// One searchable palette entry: a command, a target or a settings page, already localized.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteEntry {
    label: String,
    keywords: String,
    recent: Option<u8>,
    brevity: u32,
}

impl PaletteEntry {
    /// Folds `label` and `keywords` for matching. `recent` is the entry's place in the recently
    /// used list (0 = most recent); only the first 20 count.
    #[must_use]
    pub fn new(label: &str, keywords: &str, recent: Option<u8>) -> Self {
        let label = search_text(label, TEXT_MAX);
        let chars = u32::try_from(label.chars().count()).unwrap_or(u32::MAX);
        Self {
            brevity: 100u32.saturating_sub(chars) / 4,
            keywords: search_text(keywords, TEXT_MAX),
            recent: recent.filter(|&r| r < RECENT_RANKS),
            label,
        }
    }
}

/// How well an entry matches. Higher is better.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Score(pub u32);

/// Ranks `entries` for `query` into `out` as `(entry index, score)`, best first, ties in entry
/// order. Entries that don't match are left out. An empty query lists every entry: recent ones
/// first, by recency, then the rest in entry order.
pub fn rank(query: &str, entries: &[PaletteEntry], out: &mut Vec<(u32, Score)>) {
    out.clear();
    let query = search_text(query, TEXT_MAX);
    for (i, e) in (0u32..).zip(entries) {
        let score = if query.is_empty() {
            Some(e.recent.map_or(0, |r| EMPTY_RECENT - u32::from(r)))
        } else {
            score(&query, e)
        };
        if let Some(s) = score {
            out.push((i, Score(s)));
        }
    }
    out.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
}

fn score(query: &str, e: &PaletteEntry) -> Option<u32> {
    let mut total = 0u32;
    for term in query.split(' ') {
        let label = term_score(term, &e.label, true);
        let keyword = term_score(term, &e.keywords, false).map(|s| s / KEYWORD_DIVISOR);
        total = total.saturating_add(label.max(keyword)?);
    }
    if e.label.starts_with(query) {
        total = total.saturating_add(WHOLE_QUERY_PREFIX);
    }
    if let Some(r) = e.recent {
        total = total.saturating_add(RECENT_TOP - 3 * u32::from(r));
    }
    Some(total.saturating_add(e.brevity))
}

/// The best way `term` matches `text`. Fuzzy kinds (initials, subsequence) only when `fuzzy`.
fn term_score(term: &str, text: &str, fuzzy: bool) -> Option<u32> {
    let mut substring = false;
    for (at, _) in text.match_indices(term) {
        if at == 0 {
            return Some(WORD_PREFIX + AT_START);
        }
        if is_word_start(text, at) {
            return Some(WORD_PREFIX);
        }
        substring = true;
    }
    if substring {
        return Some(SUBSTRING);
    }
    if !fuzzy {
        return None;
    }
    if initials(term, text) {
        return Some(INITIALS);
    }
    subsequence(term, text).then_some(SUBSEQUENCE)
}

/// A word starts at the text's start or after a character that isn't alphanumeric.
fn is_word_start(text: &str, at: usize) -> bool {
    text.get(..at)
        .and_then(|before| before.chars().next_back())
        .is_none_or(|c| !c.is_alphanumeric())
}

/// Every character of `term`, in order, starts a word of `text`.
fn initials(term: &str, text: &str) -> bool {
    let mut want = term.chars().peekable();
    let mut prev_alnum = false;
    for c in text.chars() {
        let starts = !prev_alnum && c.is_alphanumeric();
        prev_alnum = c.is_alphanumeric();
        if starts && want.next_if_eq(&c).is_some() && want.peek().is_none() {
            return true;
        }
    }
    want.peek().is_none()
}

/// Every character of `term` appears in `text`, in order.
fn subsequence(term: &str, text: &str) -> bool {
    let mut have = text.chars();
    term.chars().all(|c| have.any(|h| h == c))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(query: &str, entries: &[PaletteEntry]) -> Vec<String> {
        let mut out = Vec::new();
        rank(query, entries, &mut out);
        out.iter()
            .map(|&(i, _)| entries[i as usize].label.clone())
            .collect()
    }

    fn plain(labels: &[&str]) -> Vec<PaletteEntry> {
        labels
            .iter()
            .map(|l| PaletteEntry::new(l, "", None))
            .collect()
    }

    #[test]
    fn empty_query_lists_recent_first_then_entry_order() {
        let e = vec![
            PaletteEntry::new("Open Drivers", "", None),
            PaletteEntry::new("Remove package", "", Some(1)),
            PaletteEntry::new("Scan", "", None),
            PaletteEntry::new("Clean", "", Some(0)),
            PaletteEntry::new("Old", "", Some(25)),
        ];
        assert_eq!(
            labels("   ", &e),
            ["clean", "remove package", "open drivers", "scan", "old"]
        );
    }

    #[test]
    fn match_kinds_rank_prefix_word_substring_initials_subsequence() {
        let e = plain(&[
            "Rename profile", // initials
            "Supress",        // p before r: no match
            "Carpet",         // substring
            "Repair",         // subsequence
            "Rp tools",       // at the start
            "Open rp viewer", // at a word start
        ]);
        assert_eq!(
            labels("rp", &e),
            [
                "rp tools",
                "open rp viewer",
                "carpet",
                "rename profile",
                "repair"
            ]
        );
    }

    #[test]
    fn terms_match_in_any_order_and_all_must_match() {
        let e = plain(&["Remove package", "Remove driver", "Add package"]);
        assert_eq!(labels("package remove", &e), ["remove package"]);
        assert_eq!(labels("remove p", &e), ["remove package"]);
        assert_eq!(labels("remove zz", &e), Vec::<String>::new());
    }

    #[test]
    fn whole_query_prefix_beats_scattered_terms() {
        let e = plain(&["Package remove history", "Remove package"]);
        assert_eq!(
            labels("remove package", &e),
            ["remove package", "package remove history"]
        );
    }

    #[test]
    fn keywords_match_without_fuzzy_and_below_the_label() {
        let e = vec![
            PaletteEntry::new("Clean up", "temp files cache", None),
            PaletteEntry::new("Cache viewer", "", None),
        ];
        assert_eq!(labels("cache", &e), ["cache viewer", "clean up"]);
        // "tf" is the initials of "temp files", but keywords don't match fuzzily.
        assert_eq!(labels("tf", &e), Vec::<String>::new());
    }

    #[test]
    fn matching_folds_case_and_unicode() {
        let e = plain(&["Éditer les paramètres", "ДРАЙВЕРЫ"]);
        assert_eq!(labels("ÉDITER", &e), ["éditer les paramètres"]);
        assert_eq!(labels("драйв", &e), ["драйверы"]);
    }

    #[test]
    fn recency_breaks_close_scores_and_ties_keep_entry_order() {
        let e = vec![
            PaletteEntry::new("Scan drivers", "", None),
            PaletteEntry::new("Scan updates", "", Some(0)),
            PaletteEntry::new("Scan apps xx", "", None),
            PaletteEntry::new("Scan logs xx", "", None),
        ];
        assert_eq!(
            labels("scan", &e),
            [
                "scan updates",
                "scan drivers",
                "scan apps xx",
                "scan logs xx"
            ]
        );
    }

    #[test]
    fn out_is_replaced_on_each_call() {
        let e = plain(&["Scan", "Clean"]);
        let mut out = vec![(9, Score(9))];
        rank("scan", &e, &mut out);
        assert_eq!(out.len(), 1);
        rank("zzz", &e, &mut out);
        assert_eq!(out, Vec::<(u32, Score)>::new());
    }

    #[test]
    fn initials_need_word_starts_in_order() {
        assert!(initials("rp", "remove package"));
        assert!(initials("rpc", "remove package cache"));
        assert!(!initials("pr", "remove package"));
        assert!(!initials("rm", "remove package"));
        assert!(initials("ab", "x-a (b)"));
    }

    #[test]
    fn word_starts_follow_non_alphanumerics() {
        assert!(is_word_start("back up", 5));
        assert!(is_word_start("x-ray", 2));
        assert!(!is_word_start("backup", 4));
        assert!(is_word_start("é", 0));
    }
}
