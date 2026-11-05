//! CBS.log / DISM.log parsing (TD-0006 §5.2). Streaming, O(1) memory in the log size, never
//! panics: logs are untrusted input read by an elevated process (fuzz targets in Phase 6).
//!
//! Line shape (undocumented, tolerant parsing): `2026-10-01 12:00:00, Info  CBS  message`.
//!
//! In dism.log only the servicing stack's own lines count (timestamped, component `CBS` or
//! `CSI`): DISM writes them there when it services an offline image, while its
//! `[pid.tid] [hr] Function:(line)` probe lines are noise (TD-0006 §7, run 41670190046).

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

/// The first `HRESULT = 0x…` / `hr = 0x…` / `(hr:0x…)` / `gle=0x…` code in `text`.
#[must_use]
pub fn hresult_in(text: &str) -> Option<i32> {
    for marker in ["HRESULT = 0x", "hr = 0x", "hr: 0x", "(hr:0x", "[gle=0x"] {
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

/// Text patterns: every needle must appear in the line; the first matching row wins. They
/// stay narrow on purpose: CBS's end-of-scan summary (`CSI Payload Corruption:\t0`) names
/// the same things with a count of zero, and a bare `Payload Corrupt` matched it on a healthy
/// image (run 41670190046).
const PATTERNS: &[(&[&str], &str, Severity)] = &[
    // CSI marking the store corrupt, as an offline scan logs it to dism.log (run 41670190046):
    // `(F) Attempting to mark store corrupt with category [l:18 ml:19]'CorruptPayloadFile'[gle=…]`.
    (
        &["mark store corrupt", "'CorruptPayloadFile'"],
        "health-finding-payload-corrupt",
        Severity::Error,
    ),
    (
        &["mark store corrupt"],
        "health-finding-store-corrupt",
        Severity::Error,
    ),
    // One damaged file in the scan's detail list: `(p)\tCSI Payload Corrupt\t\t\t<component>`.
    (
        &["(p)", "CSI Payload Corrupt"],
        "health-finding-payload-corrupt",
        Severity::Warning,
    ),
];

/// The diagnosis (a Fluent id that names the next step) and severity of a known HRESULT, as
/// findings use them; `None` for a code the table doesn't know.
#[must_use]
pub fn diagnosis_for(code: u32) -> Option<(&'static str, Severity)> {
    DECODE
        .iter()
        .find(|(k, ..)| *k == code)
        .map(|(_, id, sev)| (*id, *sev))
}

/// Whether a line of `file` can be a finding at all: in dism.log, only the servicing stack's
/// timestamped CBS and CSI lines.
fn counts(file: LogFile, line: &LogLine<'_>) -> bool {
    match file {
        LogFile::Cbs => true,
        LogFile::Dism => {
            line.timestamp.is_some()
                && line
                    .component
                    .is_some_and(|c| c.eq_ignore_ascii_case("CBS") || c.eq_ignore_ascii_case("CSI"))
        }
    }
}

/// A known code first, then a pattern (carrying the line's code, if any), then an unknown
/// failure code on an error line.
fn classify(line: &LogLine<'_>) -> Option<(&'static str, Severity, Option<i32>)> {
    let code = hresult_in(line.text);
    let bits = code.map(|c| u32::from_ne_bytes(c.to_ne_bytes()));
    if let Some((id, sev)) = bits.and_then(diagnosis_for) {
        return Some((id, sev, code));
    }
    if let Some((_, id, sev)) = PATTERNS.iter().find(|(needles, ..)| {
        needles
            .iter()
            .all(|n| find_ignore_case(line.text, n).is_some())
    }) {
        return Some((id, *sev, code));
    }
    // An unknown failure code on an error line is still worth showing.
    let is_error = line.level.is_some_and(|l| l.eq_ignore_ascii_case("Error"));
    bits.filter(|b| is_error && b & 0x8000_0000 != 0)
        .map(|_| ("health-finding-unknown-error", Severity::Error, code))
}

/// Scans a whole log. Consecutive lines with the same diagnosis and code collapse into one
/// finding spanning their line range. `file` says which log it is: a dism.log counts only the
/// servicing stack's lines (see the module docs).
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
        if !counts(file, &line) {
            continue;
        }
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

/// Scans a log after its first `skip` bytes, numbering lines from the start of the log; the
/// bytes before are read only to count their lines. A `skip` inside a line drops the rest of
/// that line, since it can't be parsed. Returns the findings and the number of the first line
/// scanned.
///
/// # Errors
/// I/O errors from `reader`.
pub fn scan_after<R: BufRead>(
    mut reader: R,
    skip: u64,
    file: LogFile,
) -> std::io::Result<(Vec<Finding>, u64)> {
    let (mut lines_before, mid_line) = skip_counting_lines(&mut reader, skip)?;
    if mid_line {
        let mut rest = Vec::new();
        if read_bounded_line(&mut reader, &mut rest)? > 0 {
            lines_before += 1;
        }
    }
    let mut findings = scan(reader, file)?;
    for f in &mut findings {
        f.source.first_line += lines_before;
        f.source.last_line += lines_before;
    }
    Ok((findings, lines_before + 1))
}

/// Discards up to `n` bytes, counting the lines that end in them. Also says whether it
/// stopped inside a line.
fn skip_counting_lines<R: BufRead>(reader: &mut R, mut n: u64) -> std::io::Result<(u64, bool)> {
    let (mut lines, mut mid_line) = (0_u64, false);
    while n > 0 {
        let buf = reader.fill_buf()?;
        if buf.is_empty() {
            break;
        }
        let take = buf.len().min(usize::try_from(n).unwrap_or(usize::MAX));
        #[expect(
            clippy::naive_bytecount,
            reason = "the compiler vectorizes this loop; not worth a dependency"
        )]
        let ends = buf[..take].iter().filter(|&&b| b == b'\n').count();
        lines += u64::try_from(ends).unwrap_or(u64::MAX);
        mid_line = buf[take - 1] != b'\n';
        reader.consume(take);
        n -= u64::try_from(take).unwrap_or(n);
    }
    Ok((lines, mid_line))
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
2026-10-01 12:00:03, Info                  CSI    (p)	CSI Payload Corrupt			amd64_foo
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

    /// The runner's dism.log, written during an offline scan of a container base layer
    /// (run 41670190046): one CSI line explains the repairable state; the probe lines and
    /// DISM's own lines don't count.
    #[test]
    fn an_offline_scan_is_explained_by_dism_log() {
        let log = "\
[7440.8596] [0x80070002] FIOReadFileIntoBuffer:(1454): The system cannot find the file specified.
[7440.8596] [0xc142011c] UnmarshallImageHandleFromDirectory:(641)
[7440.8596] [0xc142011c] WIMGetMountedImageHandle:(2910)
2026-11-03 16:25:01, Error                 CSI    000000b2@2026/11/3:16:25:01.867 (F) Attempting to mark store corrupt with category [l:18 ml:19]'CorruptPayloadFile'[gle=0x80004005]
2026-11-03 16:25:09, Info                  DISM   PID=7440 TID=2192 Scratch directory set to 'C:\\Users\\RUNNER~1\\AppData\\Local\\Temp\\'. - CDISMManager::put_ScratchDir
2026-11-03 16:25:10, Error                 DISM   DISM Package Manager: PID=7440 TID=2192 Failed to restore. - CDISMPackageManager::RestoreHealth(hr:0x800f081f)
";
        let f = scan(log.as_bytes(), LogFile::Dism).unwrap();
        assert_eq!(f.len(), 1, "{f:?}");
        assert_eq!(f[0].diagnosis, "health-finding-payload-corrupt");
        assert_eq!(f[0].severity, Severity::Error);
        let e_fail = i32::from_ne_bytes(0x8000_4005_u32.to_ne_bytes());
        assert_eq!(f[0].code.map(|c| c.code), Some(e_fail));
        assert_eq!(
            f[0].source,
            LogRef {
                file: LogFile::Dism,
                first_line: 4,
                last_line: 4
            }
        );
        // The same lines in a CBS.log all count, DISM's `hr:` included.
        let all = scan(log.as_bytes(), LogFile::Cbs).unwrap();
        assert_eq!(all.len(), 2, "{all:?}");
        assert_eq!(all[1].diagnosis, "health-finding-source-missing");
    }

    /// CBS's summary at the end of a scan that found nothing, from the healthy online runner
    /// (run 41670190046): a bare `Payload Corrupt` pattern reported it as damage.
    #[test]
    fn a_clean_scan_summary_is_not_a_finding() {
        let log = "\
2026-11-03 16:15:40, Info                  CSI    0000000a@2026/11/3:16:15:40.666 Corruption detection complete. numCorruptions = 0, Disp = 1.
2026-11-03 16:15:47, Info                  CBS    Summary:
2026-11-03 16:15:47, Info                  CBS    Total Detected Corruption:\t0
2026-11-03 16:15:47, Info                  CBS    \tCSI Manifest Corruption:\t0
2026-11-03 16:15:47, Info                  CBS    \tCSI Metadata Corruption:\t0
2026-11-03 16:15:47, Info                  CBS    \tCSI Payload Corruption:\t0
2026-11-03 16:15:47, Info                  CBS    Total Repaired Corruption:\t0
2026-10-30 07:18:29, Info                  CBS    CbsCoreStopIdleProcessing is not implemented. [HRESULT = 0x80004001 - E_NOTIMPL]
";
        assert_eq!(scan(log.as_bytes(), LogFile::Cbs).unwrap(), Vec::new());
        assert_eq!(scan(log.as_bytes(), LogFile::Dism).unwrap(), Vec::new());
    }

    #[test]
    fn skipping_counts_whole_lines() {
        let mut r = std::io::Cursor::new(b"a\nbb\nccc\n".to_vec());
        assert_eq!(skip_counting_lines(&mut r, 5).unwrap(), (2, false));
        let mut rest = String::new();
        std::io::Read::read_to_string(&mut r, &mut rest).unwrap();
        assert_eq!(rest, "ccc\n");
        let mut r = std::io::Cursor::new(b"a\nbb\nccc\n".to_vec());
        assert_eq!(skip_counting_lines(&mut r, 4).unwrap(), (1, true));
        let mut r = std::io::Cursor::new(b"a\nb".to_vec());
        assert_eq!(skip_counting_lines(&mut r, 100).unwrap(), (1, true));
    }

    #[test]
    fn scanning_after_a_point_numbers_lines_from_the_top() {
        let log = "\
2026-10-01 12:00:00, Error                 CBS    old [HRESULT = 0x800f081f]
2026-10-01 12:00:01, Info                  CBS    old
2026-10-01 12:00:02, Error                 CBS    new [HRESULT = 0x80070005]
";
        let boundary = u64::try_from(log.find("2026-10-01 12:00:02").unwrap()).unwrap();
        let (f, first) = scan_after(log.as_bytes(), boundary, LogFile::Cbs).unwrap();
        assert_eq!(first, 3);
        assert_eq!(f.len(), 1, "{f:?}");
        assert_eq!(f[0].diagnosis, "health-finding-access-denied");
        assert_eq!((f[0].source.first_line, f[0].source.last_line), (3, 3));
        // Inside the first line: its rest is dropped, and the old line 1 isn't a finding.
        let (f, first) = scan_after(log.as_bytes(), 5, LogFile::Cbs).unwrap();
        assert_eq!((first, f.len()), (2, 1));
        assert_eq!(f[0].source.first_line, 3);
        // Nothing to skip, and a skip past the end.
        assert_eq!(
            scan_after(log.as_bytes(), 0, LogFile::Cbs).unwrap().0.len(),
            2
        );
        assert_eq!(
            scan_after(log.as_bytes(), 1 << 20, LogFile::Cbs).unwrap(),
            (Vec::new(), 4)
        );
    }

    #[test]
    fn marking_the_store_corrupt_names_the_category() {
        let line = |category: &str| {
            format!(
                "2026-11-03 16:25:01, Error                 CSI    000000b2 (F) Attempting to \
                 mark store corrupt with category [l:18 ml:19]'{category}'[gle=0x80004005]\n"
            )
        };
        let payload = scan(line("CorruptPayloadFile").as_bytes(), LogFile::Dism).unwrap();
        assert_eq!(payload[0].diagnosis, "health-finding-payload-corrupt");
        let other = scan(line("CorruptComponentValue").as_bytes(), LogFile::Dism).unwrap();
        assert_eq!(other[0].diagnosis, "health-finding-store-corrupt");
        let detail = "(p)\tCSI Payload Corrupt\t\t\tamd64_foo_31bf3856ad364e35\\foo.dll\n";
        let listed = scan(detail.as_bytes(), LogFile::Cbs).unwrap();
        assert_eq!(listed[0].diagnosis, "health-finding-payload-corrupt");
        assert_eq!(listed[0].severity, Severity::Warning);
        let not_found = i32::from_ne_bytes(0x8007_0002_u32.to_ne_bytes());
        assert_eq!(hresult_in("x [gle=0x80070002]"), Some(not_found));
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
