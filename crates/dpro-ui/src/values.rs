//! Sizes, days and moments in the user's formats (TD-0009 A2.5, slice 5b3).
//!
//! A table's value cells hold numbers ([`Cells::size`](crate::Cells::size),
//! [`date`](crate::Cells::date), [`time`](crate::Cells::time)). The window's [`Formats`] turn them
//! into the text a cell shows and a client reads: the user's on Windows (`platform`), or
//! [`InvariantFormats`] for tests, benches, and a window whose host hasn't set the user's yet.
//!
//! Each table keeps the text of the values it showed ([`ValueCache`]), so a frame that changes
//! nothing formats nothing, and rows that come back into view neither format nor allocate. New
//! formats ([`Ui::set_formats`](crate::Ui::set_formats)) drop what was kept.

use std::{
    collections::HashMap,
    fmt,
    hash::{BuildHasherDefault, Hash, Hasher},
    mem,
    rc::Rc,
};

use dpro_core::units::{Bytes, UnixMillis};

/// How a window shows sizes, days and moments. Each method appends to `out`.
pub trait Formats {
    /// `bytes` as the user reads a size: [`format_size`] with the user's decimal separator.
    fn size(&self, bytes: Bytes, out: &mut String);
    /// A calendar day, held as its midnight UTC, in the user's short date format. No time zone
    /// applies: a program installed on the 6th was installed on the 6th wherever the user is.
    fn date(&self, day: UnixMillis, out: &mut String);
    /// A moment in local time: the user's short date, a space, and their short time.
    fn time(&self, t: UnixMillis, out: &mut String);
}

/// Bytes, kilobytes, megabytes, gigabytes, terabytes, petabytes and exabytes: a `u64` of bytes
/// is under 16 EB, so a size never needs a fourth integer digit.
const UNITS: usize = 7;

/// A window's formats, shared with each table's build.
#[derive(Clone)]
pub(crate) struct Shared(pub(crate) Rc<dyn Formats>);

impl fmt::Debug for Shared {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Formats")
    }
}

/// The size units' labels, from the app's catalog: each unit's text before and after the number.
/// The default has none: the number alone.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SizeUnits {
    units: [(String, String); UNITS],
}

impl SizeUnits {
    /// One pattern per unit, bytes to exabytes, with `{}` where the number goes (`{} KB`). A
    /// pattern without `{}` follows the number.
    #[must_use]
    pub fn new(patterns: [&str; UNITS]) -> Self {
        Self {
            units: patterns.map(|p| match p.split_once("{}") {
                Some((before, after)) => (before.to_owned(), after.to_owned()),
                None => (String::new(), p.to_owned()),
            }),
        }
    }
}

/// Appends `bytes` with three significant digits in base 1024, as Explorer shows sizes:
/// `999 B`, `0.98 KB` (1,000 bytes), `1.50 KB`, `10.0 KB`, `100 KB`, up to `16.0 EB`.
///
/// Under 1,000 bytes, whole bytes. Otherwise the unit is the largest that leaves the number
/// under 1,000 after rounding half up, once, in integers: a size never reads 1,000 of a unit, and
/// never needs a group separator. `decimal` is the locale's decimal separator.
pub fn format_size(bytes: Bytes, decimal: &str, units: &SizeUnits, out: &mut String) {
    // A unit is 2^(10 × unit) bytes, so dividing by one is a shift.
    let bytes = u128::from(bytes.0);
    let mut unit: u32 = 0;
    if bytes >= 1_000 {
        unit = 1;
        while unit < LAST_UNIT && bytes >= 1_000 << (10 * unit) {
            unit += 1;
        }
    }
    // Digits after the separator, for three significant ones.
    let mut digits: u32 = if unit == 0 || bytes >= 100 << (10 * unit) {
        0
    } else if bytes >= 10 << (10 * unit) {
        1
    } else {
        2
    };
    let mut n = rounded(bytes, unit, digits);
    while n >= 1_000 && unit > 0 {
        if digits > 0 {
            // 9.995 is 10.0, and 99.95 is 100.
            digits -= 1;
        } else if unit < LAST_UNIT {
            // 999.5 KB is most of a megabyte.
            unit += 1;
            digits = 2;
        } else {
            break;
        }
        n = rounded(bytes, unit, digits);
    }
    let (before, after) = &units.units[unit as usize];
    out.push_str(before);
    // Under 1,000 (160 at most in exabytes).
    let n = u64::try_from(n).unwrap_or(u64::MAX);
    let scale = 10_u64.pow(digits);
    push_digits(out, n / scale, 1);
    if digits > 0 {
        out.push_str(decimal);
        push_digits(out, n % scale, digits as usize);
    }
    out.push_str(after);
}

/// Exabytes, the last unit.
const LAST_UNIT: u32 = 6;

/// `bytes` in units of 2^(10 × `unit`), with `digits` digits after the point, as an integer:
/// rounded half up.
fn rounded(bytes: u128, unit: u32, digits: u32) -> u128 {
    let shift = 10 * unit;
    let half = (1_u128 << shift) >> 1;
    (bytes * u128::from(10_u32.pow(digits)) + half) >> shift
}

/// Appends `n` in decimal, at least `width` digits long (zeros first). Formatting machinery
/// would cost several times as much, on every value a scroll brings into view.
#[inline]
fn push_digits(out: &mut String, mut n: u64, width: usize) {
    // Zeros already where the width needs them.
    let mut buf = [b'0'; 20];
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + u8::try_from(n % 10).unwrap_or(0);
        n /= 10;
        if n == 0 {
            break;
        }
    }
    let i = i.min(buf.len().saturating_sub(width));
    // A char at a time: checking a few bytes for UTF-8 costs more.
    out.extend(buf[i..].iter().map(|&b| char::from(b)));
}

/// Formats with no locale: `.` for the decimal separator, ISO 8601 days (`2026-11-06`) and
/// 24-hour moments in UTC (`2026-11-06 15:04`). For tests, benches, and a window whose host
/// hasn't set the user's formats.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvariantFormats {
    units: SizeUnits,
    decimal: String,
}

impl Default for InvariantFormats {
    fn default() -> Self {
        Self::new(SizeUnits::default())
    }
}

impl InvariantFormats {
    #[must_use]
    pub fn new(units: SizeUnits) -> Self {
        Self {
            units,
            decimal: ".".to_owned(),
        }
    }

    /// With `decimal` as the decimal separator.
    #[must_use]
    pub fn with_decimal(mut self, decimal: &str) -> Self {
        decimal.clone_into(&mut self.decimal);
        self
    }
}

impl Formats for InvariantFormats {
    fn size(&self, bytes: Bytes, out: &mut String) {
        format_size(bytes, &self.decimal, &self.units, out);
    }

    fn date(&self, day: UnixMillis, out: &mut String) {
        let (y, m, d) = civil(day.0 / DAY_MS);
        push_digits(out, y, 4);
        out.push('-');
        push_digits(out, m, 2);
        out.push('-');
        push_digits(out, d, 2);
    }

    fn time(&self, t: UnixMillis, out: &mut String) {
        self.date(t, out);
        let minutes = t.0 % DAY_MS / 60_000;
        out.push(' ');
        push_digits(out, minutes / 60, 2);
        out.push(':');
        push_digits(out, minutes % 60, 2);
    }
}

const DAY_MS: u64 = 86_400_000;

/// The proleptic Gregorian year, month and day `days` after 1970-01-01 (Howard Hinnant's
/// `civil_from_days`, for days on or after the epoch).
fn civil(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + u64::from(m <= 2);
    (y, m, d)
}

/// A value cell's value, by kind: the cache's key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Value {
    Size(u64),
    Date(u64),
    Time(u64),
}

impl Hash for Value {
    /// One `u64`, the value with its kind in the top bits, so a lookup mixes once. Values whose
    /// kinds collide are told apart by `Eq`.
    fn hash<H: Hasher>(&self, state: &mut H) {
        let (kind, v) = match *self {
            Self::Size(v) => (0, v),
            Self::Date(v) => (1, v),
            Self::Time(v) => (2, v),
        };
        state.write_u64(v ^ (kind << 62));
    }
}

/// Text longer than this isn't kept (a user can make their short date picture as long as they
/// like): it's formatted at each build instead. A kept value's length fits a `u8`.
const KEEP: usize = 128;

/// Where a value's text is in the cache's text, and the build that last showed it.
#[derive(Clone, Copy, Debug)]
struct Kept {
    start: u32,
    len: u8,
    build: u32,
}

/// The text a table showed for its values, by value (TD-0009 A2.5): formatted once, kept while
/// the rows that show it might come back.
///
/// The kept text is one string, end to end, so keeping a value allocates nothing of its own.
/// Its room is made at the first build. When it's full, it keeps only what the current build
/// showed, copied into the spare string, which takes the text's place; and it makes room for
/// twice as much only if that build alone showed more than half of it. The map never holds more
/// than half its capacity, so the tombstones a purge leaves are cleared in place (hashbrown
/// rehashes without allocating below half): after a window's first frames, keeping text
/// allocates nothing.
#[derive(Debug, Default)]
pub(crate) struct ValueCache {
    map: HashMap<Value, Kept, BuildHasherDefault<ValueHasher>>,
    /// The kept values' text.
    text: String,
    /// What a purge keeps of `text`, before the two change places.
    spare: String,
    /// The formats' generation the text is in.
    generation: u32,
    /// The table's builds: stamps what each one showed.
    build: u32,
    /// How many values it keeps before a purge: half its capacity.
    limit: usize,
}

/// Values a table keeps at first: rows in view and half a view either side, times a few value
/// columns, with room to scroll back.
const FIRST_ROOM: usize = 512;

/// Room for each value's text at first: a size (`1.50 KB`) or a day (`11/6/2026`) is shorter.
const FIRST_TEXT: usize = 16;

impl ValueCache {
    /// A build starts, with the window's formats at `generation`: new formats drop the old text.
    pub(crate) fn begin(&mut self, generation: u32) {
        if self.generation != generation {
            self.map.clear();
            self.text.clear();
            self.generation = generation;
        }
        if self.limit == 0 {
            self.map.reserve(FIRST_ROOM);
            self.limit = self.map.capacity() / 2;
            self.text.reserve(self.limit * FIRST_TEXT);
        }
        self.build = self.build.wrapping_add(1);
    }

    /// Appends `v`'s text to `out`: kept, or formatted by `formats` (and kept unless it's long).
    /// Inlined into the view's row closure: on most frames every value is kept.
    #[inline]
    pub(crate) fn push(&mut self, v: Value, formats: &dyn Formats, out: &mut String) {
        if let Some(kept) = self.map.get_mut(&v) {
            kept.build = self.build;
            out.push_str(text_of(&self.text, *kept));
        } else {
            self.format(v, formats, out);
        }
    }

    /// [`push`](Self::push) for a value it hasn't kept.
    #[inline(never)]
    fn format(&mut self, v: Value, formats: &dyn Formats, out: &mut String) {
        let start = out.len();
        match v {
            Value::Size(b) => formats.size(Bytes(b), out),
            Value::Date(t) => formats.date(UnixMillis(t), out),
            Value::Time(t) => formats.time(UnixMillis(t), out),
        }
        let text = &out[start..];
        if text.len() > KEEP {
            return;
        }
        let Ok(len) = u8::try_from(text.len()) else {
            return;
        };
        if self.map.len() >= self.limit {
            self.purge();
        }
        // Never fails: a purge keeps the text under `limit` values of `KEEP` bytes.
        let Ok(start) = u32::try_from(self.text.len()) else {
            return;
        };
        self.text.push_str(text);
        self.map.insert(
            v,
            Kept {
                start,
                len,
                build: self.build,
            },
        );
    }

    /// Keeps what this build showed, and makes room if that's more than half the limit.
    fn purge(&mut self) {
        let build = self.build;
        let (text, spare) = (&self.text, &mut self.spare);
        spare.clear();
        self.map.retain(|_, k| {
            if k.build != build {
                return false;
            }
            let Ok(start) = u32::try_from(spare.len()) else {
                return false;
            };
            spare.push_str(text_of(text, *k));
            k.start = start;
            true
        });
        mem::swap(&mut self.text, &mut self.spare);
        if self.map.len() * 2 > self.limit {
            self.map.reserve(self.limit * 2);
            self.limit = self.map.capacity() / 2;
        }
    }

    /// The values kept.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.map.len()
    }
}

/// A kept value's text, in the cache's `text`. Inlined, as [`ValueCache::push`] is.
#[inline]
fn text_of(text: &str, kept: Kept) -> &str {
    let start = kept.start as usize;
    text.get(start..start + usize::from(kept.len)).unwrap_or("")
}

/// A multiply, folded: a value's low bits are often zero (a day is a multiple of 86,400,000
/// ms), and hashbrown takes its buckets from the hash's low bits and its tags from the top.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ValueHasher(u64);

const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

impl Hasher for ValueHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(self.0.rotate_left(8) ^ u64::from(b));
        }
    }

    fn write_u64(&mut self, x: u64) {
        let h = x.wrapping_mul(SEED);
        self.0 = h ^ (h >> 29);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(c: &mut ValueCache, from: u64, n: u64, out: &mut String) {
        let f = InvariantFormats::default();
        for v in from..from + n {
            out.clear();
            c.push(Value::Size(v), &f, out);
        }
    }

    #[test]
    fn a_full_cache_keeps_what_this_build_showed() {
        let mut c = ValueCache::default();
        let mut out = String::new();
        c.begin(0);
        let limit = c.limit;
        let cap = c.map.capacity();
        fill(&mut c, 0, limit as u64, &mut out);
        c.begin(0);
        // The next build shows ten new values: the old ones go when it's full.
        fill(&mut c, 1_000_000, 10, &mut out);
        assert_eq!(c.len(), 10);
        assert_eq!((c.limit, c.map.capacity()), (limit, cap), "no new room");
    }

    #[test]
    fn a_build_that_shows_more_than_half_makes_room() {
        let mut c = ValueCache::default();
        let mut out = String::new();
        c.begin(0);
        let limit = c.limit;
        fill(&mut c, 0, limit as u64 + 1, &mut out);
        assert!(c.limit > limit);
        assert_eq!(c.len(), limit + 1);
    }

    #[test]
    fn new_formats_drop_the_kept_text() {
        let mut c = ValueCache::default();
        let mut out = String::new();
        c.begin(0);
        fill(&mut c, 0, 5, &mut out);
        c.begin(1);
        assert_eq!(c.len(), 0);
    }
}
