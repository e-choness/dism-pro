//! Case folding shared by the palette and the table filter. Both sides of every comparison go
//! through the same per-character lowercase, so a query always folds like the text it searches
//! (no context-dependent forms such as `str::to_lowercase`'s final sigma).

/// Appends `s` lowercased to `out`, with an ASCII fast path.
pub(crate) fn lower_into(s: &str, out: &mut String) {
    for c in s.chars() {
        if c.is_ascii() {
            out.push(c.to_ascii_lowercase());
        } else {
            out.extend(c.to_lowercase());
        }
    }
}

/// Lowercases `s`, collapses whitespace runs to one space, trims, and keeps at most `max` bytes
/// (cut at a character boundary). Control characters count as whitespace.
pub(crate) fn search_text(s: &str, max: usize) -> String {
    let mut out = String::with_capacity(s.len().min(max));
    let mut space = false;
    for c in s.chars() {
        if c.is_whitespace() || c.is_control() {
            space = !out.is_empty();
            continue;
        }
        let before = out.len();
        if space {
            out.push(' ');
            space = false;
        }
        if c.is_ascii() {
            out.push(c.to_ascii_lowercase());
        } else {
            out.extend(c.to_lowercase());
        }
        if out.len() > max {
            out.truncate(before);
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowercases_unicode_per_character() {
        let mut s = String::new();
        lower_into("ÉDITER Σ Abc", &mut s);
        assert_eq!(s, "éditer σ abc");
    }

    #[test]
    fn search_text_collapses_whitespace_and_controls() {
        assert_eq!(
            search_text("  Remove\t\u{7}  Package \n", 64),
            "remove package"
        );
        assert_eq!(search_text("", 64), "");
    }

    #[test]
    fn search_text_cuts_at_a_character_boundary() {
        assert_eq!(search_text("ab éé", 5), "ab é");
        assert_eq!(search_text("abc def", 4), "abc");
    }
}
