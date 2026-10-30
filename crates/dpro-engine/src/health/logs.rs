//! CBS.log / DISM.log parsing (TD-0006 §5.2). Streaming, O(1) memory in the log size, never
//! panics: logs are untrusted input read by an elevated process (fuzz targets in Phase 6).
//!
//! Line shape (undocumented, tolerant parsing): `2026-10-01 12:00:00, Info  CBS  message`.

use std::io::BufRead;

use dpro_core::{
    OsError,
    health::{Finding, LogFile, LogRef, Severity},
};

/// Longest line kept; longer lines are truncated (TD-0006 §8).
pub const MAX_LINE: usize = 64 * 1024;

/// One parsed line, borrowing the line buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine<'a> {
    pub timestamp: Option<&'a str>,
    pub level: Option<&'a str>,
    pub component: Option<&'a str>,
    pub text: &'a str,
}

/// Splits a line into its fields; anything that doesn't look like the usual shape becomes
/// a line with only `text`.
#[must_use]
pub fn parse_line(line: &str) -> LogLine<'_> {
    let unparsed = LogLine {
        timestamp: None,
        level: None,
        component: None,
        text: line,
    };
    let Some((ts, rest)) = line.split_once(", ") else {
        return unparsed;
    };
    if !looks_like_timestamp(ts) {
        return unparsed;
    }
    let rest = rest.trim_start();
    let Some((level, rest)) = rest.split_once(char::is_whitespace) else {
        return LogLine {
            timestamp: Some(ts),
            level: Some(rest),
            component: None,
            text: "",
        };
    };
    let rest = rest.trim_start();
    let (component, text) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    LogLine {
        timestamp: Some(ts),
        level: Some(level),
        component: Some(component),
        text: text.trim_start(),
    }
}

fn looks_like_timestamp(s: &str) -> bool {
    // YYYY-MM-DD HH:MM:SS
    let b = s.as_bytes();
    b.len() == 19
        && b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            10 => *c == b' ',
            13 | 16 => *c == b':',
            _ => c.is_ascii_digit(),
        })
}

/// The first `HRESULT = 0x…` / `hr = 0x…` code in `text`.
#[must_use]
pub fn hresult_in(text: &str) -> Option<i32> {
    for marker in ["HRESULT = 0x", "hr = 0x", "hr: 0x"] {
        if let Some(pos) = find_ignore_case(text, marker) {
            let digits = text.get(pos + marker.len()..)?.get(..8)?;
            let value = u32::from_str_radix(digits, 16).ok()?;
            return Some(i32::from_ne_bytes(value.to_ne_bytes()));
        }
    }
    None
}

fn find_ignore_case(haystack: &str, needle: &str) -> Option<usize> {
    let (h, n) = (haystack.as_bytes(), needle.as_bytes());
    h.windows(n.len()).position(|w| w.eq_ignore_ascii_case(n))
}

/// Known codes → diagnosis (TD-0006 §12: hand-curated; every entry needs verification
/// against Microsoft Learn and is listed in CAVEATS.md).
const DECODE: &[(u32, &str, Severity)] = &[
    (
        0x800F_081F,
        "health-finding-source-missing",
        Severity::Error,
    ),
    (
        0x800F_0906,
        "health-finding-download-failed",
        Severity::Error,
    ),
    (
        0x800F_0907,
        "health-finding-policy-blocks-wu",
        Severity::Error,
    ),
    (0x800F_0831, "health-finding-store-corrupt", Severity::Error),
    (0x8007_3712, "health-finding-store-corrupt", Severity::Error),
    (0x8007_0005, "health-finding-access-denied", Severity::Error),
];

/// Text patterns without an HRESULT (verify on real logs).
const PATTERNS: &[(&str, &str, Severity)] = &[(
    "Payload Corrupt",
    "health-finding-payload-corrupt",
    Severity::Warning,
)];

fn classify(line: &LogLine<'_>) -> Option<(&'static str, Severity, Option<i32>)> {
    let code = hresult_in(line.text);
    if let Some(c) = code {
        let bits = u32::from_ne_bytes(c.to_ne_bytes());
        if let Some((_, id, sev)) = DECODE.iter().find(|(k, ..)| *k == bits) {
            return Some((*id, *sev, code));
        }
        // An unknown failure code on an error line is still worth showing.
        let is_error = line.level.is_some_and(|l| l.eq_ignore_ascii_case("Error"));
        if is_error && bits & 0x8000_0000 != 0 {
            return Some(("health-finding-unknown-error", Severity::Error, code));
        }
    }
    PATTERNS
        .iter()
        .find(|(p, ..)| find_ignore_case(line.text, p).is_some())
        .map(|(_, id, sev)| (*id, *sev, code))
}

/// Scans a whole log. Consecutive lines with the same diagnosis and code collapse into one
/// finding spanning their line range.
///
/// # Errors
/// I/O errors from `reader`.
pub fn scan<R: BufRead>(mut reader: R, file: LogFile) -> std::io::Result<Vec<Finding>> {
    let mut findings: Vec<Finding> = Vec::new();
    let mut buf = Vec::with_capacity(512);
    let mut line_no = 0_u64;
    loop {
        buf.clear();
        if read_bounded_line(&mut reader, &mut buf)? == 0 {
            break;
        }
        line_no += 1;
        let text = String::from_utf8_lossy(&buf);
        let line = parse_line(text.trim_end_matches(['\r', '\n']));
        let Some((diagnosis, severity, code)) = classify(&line) else {
            continue;
        };
        let code = code.map(|c| OsError {
            code: c,
            api: "CBS",
        });
        if let Some(last) = findings.last_mut()
            && last.diagnosis == diagnosis
            && last.code == code
            && last.source.last_line + 1 == line_no
        {
            last.source.last_line = line_no;
            continue;
        }
        findings.push(Finding {
            severity,
            diagnosis,
            code,
            source: LogRef {
                file,
                first_line: line_no,
                last_line: line_no,
            },
        });
    }
    Ok(findings)
}

/// Reads up to and including `\n`, keeping at most [`MAX_LINE`] bytes (the rest of an
/// over-long line is consumed and dropped). Returns the bytes consumed.
fn read_bounded_line<R: BufRead>(reader: &mut R, out: &mut Vec<u8>) -> std::io::Result<usize> {
    let mut consumed = 0;
    loop {
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            return Ok(consumed);
        }
        let (take, done) = match chunk.iter().position(|&b| b == b'\n') {
            Some(i) => (i + 1, true),
            None => (chunk.len(), false),
        };
        let room = MAX_LINE.saturating_sub(out.len());
        out.extend_from_slice(&chunk[..take.min(room)]);
        reader.consume(take);
        consumed += take;
        if done {
            return Ok(consumed);
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn parses_the_usual_shape() {
        let l = parse_line(
            "2026-10-01 12:00:00, Info                  CBS    Loaded Servicing Stack v10.0",
        );
        assert_eq!(l.timestamp, Some("2026-10-01 12:00:00"));
        assert_eq!(l.level, Some("Info"));
        assert_eq!(l.component, Some("CBS"));
        assert_eq!(l.text, "Loaded Servicing Stack v10.0");
        assert_eq!(parse_line("garbage").text, "garbage");
        assert_eq!(parse_line("2026-13, x").timestamp, None);
    }

    #[test]
    fn finds_and_collapses_known_codes() {
        let log = "\
2026-10-01 12:00:00, Info                  CBS    Starting
2026-10-01 12:00:01, Error                 CBS    Failed to resolve package [HRESULT = 0x800f081f - CBS_E_SOURCE_MISSING]
2026-10-01 12:00:01, Error                 CBS    Failed again [HRESULT = 0x800f081f - CBS_E_SOURCE_MISSING]
2026-10-01 12:00:02, Error                 CBS    Something else [HRESULT = 0x80041234 - UNKNOWN]
2026-10-01 12:00:03, Info                  CSI    (p)	CSI Payload Corrupt  amd64_foo
";
        let f = scan(log.as_bytes(), LogFile::Cbs).unwrap();
        assert_eq!(f.len(), 3, "{f:?}");
        assert_eq!(f[0].diagnosis, "health-finding-source-missing");
        assert_eq!((f[0].source.first_line, f[0].source.last_line), (2, 3));
        assert_eq!(f[1].diagnosis, "health-finding-unknown-error");
        assert_eq!(f[2].diagnosis, "health-finding-payload-corrupt");
    }

    #[test]
    fn over_long_lines_are_bounded() {
        let mut log = vec![b'x'; MAX_LINE * 3];
        log.extend_from_slice(b"\nnext [HRESULT = 0x80070005]\n");
        let mut r = std::io::BufReader::with_capacity(1024, log.as_slice());
        let mut buf = Vec::new();
        assert_eq!(
            read_bounded_line(&mut r, &mut buf).unwrap(),
            MAX_LINE * 3 + 1
        );
        assert_eq!(buf.len(), MAX_LINE);
        let f = scan(log.as_slice(), LogFile::Cbs).unwrap();
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].source.first_line, 2);
    }

    proptest! {
        #[test]
        fn never_panics_and_counts_lines(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
            let findings = scan(bytes.as_slice(), LogFile::Dism).unwrap();
            let lines = bytes.split(|&b| b == b'\n').count() as u64;
            for f in findings {
                prop_assert!(f.source.first_line <= f.source.last_line);
                prop_assert!(f.source.last_line <= lines);
            }
        }
    }
}
