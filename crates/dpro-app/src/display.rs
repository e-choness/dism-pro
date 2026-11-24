//! Untrusted text as the UI shows it (TD-0009 §8's `display_text()`, A3.8), in its identifier and
//! path form: the names and paths the in-app picker lists (slice 6e) and, as they land, package
//! names, volume labels and the rest §8 lists.
//!
//! - **Control characters** (C0, DEL, C1) and **bidirectional controls** (the embeddings and
//!   overrides U+202A–U+202E, the isolates U+2066–U+2069) show as escapes, `<U+202E>`, so a name
//!   can't hide a character or reorder itself to disguise its extension ("Trojan Source").
//!   `<` and `>` can't be in a Windows file name, so an escape can't be mistaken for one, and
//!   [`real_text`] turns the escapes back into the characters they stand for: a path the user
//!   sees, edits or opens is the real one.
//! - **A 64 KiB cap** ([`MAX_TEXT`]) on what one text shows, cut at a character boundary and never
//!   inside an escape.
//! - **Lossy UTF-16** decoding is the caller's, before this (`OsStr::to_string_lossy`).
//!
//! Text with nothing to escape is borrowed, so a clean name costs no allocation. §8's prose form
//! (the text wrapped in an isolate) comes with A7's formatter, its first user.

use std::{borrow::Cow, fmt::Write as _};

/// The most bytes one text shows (TD-0009 §8).
pub const MAX_TEXT: usize = 64 * 1024;

/// An escape's length: `<U+` and `>` around four hex digits (every escaped character is in the
/// Basic Multilingual Plane).
const ESCAPE_LEN: usize = 8;

/// Whether `c` shows as an escape in an identifier or a path: a control character, or a
/// bidirectional embedding, override or isolate.
#[must_use]
pub fn escaped(c: char) -> bool {
    c.is_control() || matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// `text` as an identifier or a path shows (the module's rules): borrowed when there's nothing to
/// escape and it's under the cap.
#[must_use]
pub fn display_text(text: &str) -> Cow<'_, str> {
    if text.len() <= MAX_TEXT && !text.chars().any(escaped) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len().min(MAX_TEXT));
    push_display(text, &mut out);
    Cow::Owned(out)
}

/// Appends `text` as [`display_text`] shows it to `out`, up to [`MAX_TEXT`] bytes of it, with no
/// allocation beyond `out`'s growth.
pub fn push_display(text: &str, out: &mut String) {
    let limit = out.len().saturating_add(MAX_TEXT);
    for c in text.chars() {
        let len = if escaped(c) { ESCAPE_LEN } else { c.len_utf8() };
        if out.len() + len > limit {
            break;
        }
        if escaped(c) {
            // ignore: writing to a String can't fail.
            let _ = write!(out, "<U+{:04X}>", u32::from(c));
        } else {
            out.push(c);
        }
    }
}

/// The text [`display_text`] showed as `shown`: each escape of a character it escapes (`<U+202E>`)
/// turned back into that character, anything else kept as it is. Borrowed when there's no escape.
#[must_use]
pub fn real_text(shown: &str) -> Cow<'_, str> {
    if !shown.contains("<U+") {
        return Cow::Borrowed(shown);
    }
    let mut out = String::with_capacity(shown.len());
    let mut rest = shown;
    while let Some(at) = rest.find("<U+") {
        let (before, from) = rest.split_at(at);
        out.push_str(before);
        if let Some(c) = unescape(from) {
            out.push(c);
            rest = from.get(ESCAPE_LEN..).unwrap_or("");
        } else {
            out.push('<');
            rest = from.get(1..).unwrap_or("");
        }
    }
    out.push_str(rest);
    Cow::Owned(out)
}

/// The character the escape at the start of `s` stands for, if it's one [`push_display`] writes.
fn unescape(s: &str) -> Option<char> {
    let escape = s.get(..ESCAPE_LEN)?;
    let hex = escape.strip_prefix("<U+")?.strip_suffix('>')?;
    if !hex
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b))
    {
        return None;
    }
    let c = char::from_u32(u32::from_str_radix(hex, 16).ok()?)?;
    escaped(c).then_some(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_text_is_borrowed() {
        for clean in [
            "",
            "Folder 00001",
            "C:\\Windows\\WinSxS",
            "Æble ø 日本 😀 <a>",
        ] {
            assert!(matches!(display_text(clean), Cow::Borrowed(_)), "{clean}");
            assert!(matches!(real_text(clean), Cow::Borrowed(_)), "{clean}");
        }
    }

    #[test]
    fn controls_and_bidi_controls_show_as_escapes() {
        assert_eq!(
            display_text("photo\u{202E}gpj.exe"),
            "photo<U+202E>gpj.exe",
            "the override can't reverse the extension"
        );
        assert_eq!(
            display_text("a\u{0}b\u{1b}c\u{7f}d\u{85}e\u{9b}f"),
            "a<U+0000>b<U+001B>c<U+007F>d<U+0085>e<U+009B>f"
        );
        assert_eq!(
            display_text("\u{202A}\u{202B}\u{202C}\u{202D}\u{2066}\u{2067}\u{2068}\u{2069}"),
            "<U+202A><U+202B><U+202C><U+202D><U+2066><U+2067><U+2068><U+2069>"
        );
        // Marks that aren't embeddings, overrides or isolates stay (§8's list).
        assert_eq!(display_text("a\u{200E}b"), "a\u{200E}b");
    }

    #[test]
    fn real_text_undoes_display_text_and_keeps_anything_else() {
        for s in [
            "photo\u{202E}gpj.exe",
            "C:\\a\u{1}b\\\u{2067}c\u{2069}",
            "\u{9b}x<U+",
            "x<U+202",
        ] {
            assert_eq!(real_text(&display_text(s)), s, "{s:?}");
        }
        // Text that only looks like an escape stays: not hex, lowercase, or a character that
        // isn't escaped.
        for kept in ["<U+20ZE>", "<U+202e>", "<U+0041>", "<U+D800>", "<U+202E"] {
            assert_eq!(real_text(kept), kept);
        }
        assert_eq!(real_text("<<U+202E>>"), "<\u{202E}>");
    }

    #[test]
    fn the_cap_cuts_at_a_character_and_never_inside_an_escape() {
        let long = "é".repeat(MAX_TEXT);
        let shown = display_text(&long);
        assert!(shown.len() <= MAX_TEXT && shown.len() > MAX_TEXT - 2);
        assert!(shown.chars().all(|c| c == 'é'));
        let escapes = format!("a{}", "\u{202E}".repeat(MAX_TEXT));
        let shown = display_text(&escapes);
        assert!(shown.len() <= MAX_TEXT);
        assert_eq!((shown.len() - 1) % ESCAPE_LEN, 0, "whole escapes only");
        let mut out = String::from("kept ");
        push_display(&long, &mut out);
        assert!(out.starts_with("kept ") && out.len() <= "kept ".len() + MAX_TEXT);
    }
}
