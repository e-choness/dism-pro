//! The user's formats for a table's sizes, days and moments (TD-0009 A2.5, slice 5b3).
//!
//! - **Days and moments:** `GetDateFormatEx` (`DATE_SHORTDATE`), and `GetTimeFormatEx` with the
//!   short time picture (`LOCALE_SSHORTTIME`; given none, it takes the long time format and can
//!   only drop its seconds), for the user's locale (`LOCALE_NAME_USER_DEFAULT`) with their
//!   changes to it. A text longer than the stack buffer gets one of the size Windows asks for.
//! - **Local time:** a moment is put in local time first, by the zone's rules for its year
//!   (`GetTimeZoneInformationForYear`, kept for the year last read, then
//!   `SystemTimeToTzSpecificLocalTime`); a day, held as its midnight UTC, isn't. When the user
//!   has turned daylight saving time off, the zone's standing rules, which then have none.
//!   *(verify on G1: that the year's rules don't bring it back.)*
//!   `SystemTimeToTzSpecificLocalTimeEx` would do it in one call, but Wine doesn't have it, and
//!   calling it there ends the process.
//! - **Sizes:** the decimal separator (`GetLocaleInfoEx`, `LOCALE_SDECIMAL`). dpro-ui does the
//!   rest: three significant digits never need a group separator.
//! - **Changes:** [`stamp`] hashes what the text depends on: the locale, its pictures and
//!   symbols, its calendar, and the zone. `Env` carries it, so a format
//!   changed in Settings, which broadcasts `WM_SETTINGCHANGE` with "intl", reaches the host as a
//!   new `Env`. *(verify on G1: that a format changed with `SetLocaleInfoW` and the broadcast
//!   reads back at once in a running process; a zone change, which is rarer, is read at the next
//!   `WM_SETTINGCHANGE` or `WM_TIMECHANGE`.)*
//!
//! All of it is in `kernel32`, which base Windows PE has; `StrFormatByteSizeEx` would add
//! `shlwapi`, which it hasn't.

use std::{cell::Cell, fmt, ptr};

use dpro_core::units::UnixMillis;
use windows_sys::{
    Win32::{
        Foundation::{FILETIME, SYSTEMTIME},
        Globalization::{
            DATE_SHORTDATE, GetDateFormatEx, GetLocaleInfoEx, GetTimeFormatEx, IsValidLocaleName,
            LOCALE_ICALENDARTYPE, LOCALE_S1159, LOCALE_S2359, LOCALE_SDECIMAL, LOCALE_SNAME,
            LOCALE_SSHORTDATE, LOCALE_SSHORTTIME, TIME_NOSECONDS,
        },
        System::Time::{
            DYNAMIC_TIME_ZONE_INFORMATION, FileTimeToSystemTime, GetDynamicTimeZoneInformation,
            GetTimeZoneInformationForYear, SystemTimeToTzSpecificLocalTime, TIME_ZONE_INFORMATION,
        },
    },
    core::PCWSTR,
};

use crate::wide::WideCString;

/// 100 ns ticks from 1601-01-01 (`FILETIME`'s epoch) to 1970-01-01.
const EPOCH_TICKS: u64 = 116_444_736_000_000_000;
/// Room for a formatted day or time, in UTF-16 units: Windows' own limit for a picture is 80.
const ROOM: usize = 128;
/// [`ROOM`], as the calls take it.
const ROOM_CCH: i32 = 128;

/// A locale's formats, the user's or a named one (tests), and the zone's rules as they were
/// when it was made.
#[derive(Clone)]
pub struct Locale {
    /// The locale's name, NUL-terminated; `None` for the user's (`LOCALE_NAME_USER_DEFAULT`).
    name: Option<WideCString>,
    decimal: String,
    /// The short time picture, NUL-terminated; empty if it couldn't be read, and the time is
    /// then the locale's own, without seconds.
    short_time: Vec<u16>,
    /// Daylight saving time is off ("Adjust for daylight saving time automatically"): a moment
    /// takes the zone's standing rules, which then have none, rather than its rules by year.
    dst_off: bool,
    /// The zone's rules in the year a moment was last put in, read once while the years run.
    rules: Cell<Option<(u16, Option<TIME_ZONE_INFORMATION>)>>,
}

impl fmt::Debug for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Locale")
            .field("name", &self.name)
            .field("decimal", &self.decimal)
            .field("short_time", &String::from_utf16_lossy(&self.short_time))
            .field("dst_off", &self.dst_off)
            .finish_non_exhaustive()
    }
}

impl Locale {
    /// The user's formats, as they are now.
    #[must_use]
    pub fn user() -> Self {
        Self::with(None)
    }

    /// A locale's own formats, by name (`en-US`); `None` if Windows doesn't know it.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        let wide = WideCString::new(name).ok().filter(|_| !name.is_empty())?;
        // SAFETY: `wide` is NUL-terminated and outlives the call.
        let valid = unsafe { IsValidLocaleName(wide.as_ptr()) };
        (valid != 0).then(|| Self::with(Some(wide)))
    }

    fn with(name: Option<WideCString>) -> Self {
        let mut zone = DYNAMIC_TIME_ZONE_INFORMATION::default();
        // SAFETY: `zone` is a live value of its type. If the call fails, it stays zeroed:
        // daylight saving time on.
        unsafe { GetDynamicTimeZoneInformation(&raw mut zone) };
        let mut l = Self {
            name,
            decimal: String::new(),
            short_time: Vec::new(),
            dst_off: zone.DynamicDaylightTimeDisabled,
            rules: Cell::new(None),
        };
        let mut buf = [0; ROOM];
        if let Some(units) = info(l.name_ptr(), LOCALE_SDECIMAL, &mut buf) {
            push_utf16(units, &mut l.decimal);
        }
        if l.decimal.is_empty() {
            l.decimal.push('.');
        }
        if let Some(units) = info(l.name_ptr(), LOCALE_SSHORTTIME, &mut buf)
            && !units.is_empty()
        {
            l.short_time.extend_from_slice(units);
            l.short_time.push(0);
        }
        l
    }

    fn name_ptr(&self) -> PCWSTR {
        self.name.as_ref().map_or(ptr::null(), WideCString::as_ptr)
    }

    /// The decimal separator.
    #[must_use]
    pub fn decimal(&self) -> &str {
        &self.decimal
    }

    /// Appends the calendar day `day` (its midnight UTC) in the short date format. Appends
    /// nothing for a day Windows can't show (past the year 30827).
    pub fn date(&self, day: UnixMillis, out: &mut String) {
        if let Some(st) = system_time(day) {
            self.push_date(&st, out);
        }
    }

    /// Appends the moment `t` in local time: the short date, a space and the short time.
    /// Appends nothing for a moment Windows can't show.
    pub fn time(&self, t: UnixMillis, out: &mut String) {
        if let Some(local) = system_time(t).and_then(|utc| self.local_of(&utc)) {
            self.push_moment(&local, out);
        }
    }

    /// Appends `st` as a moment: its short date, a space and its short time, or nothing.
    fn push_moment(&self, st: &SYSTEMTIME, out: &mut String) {
        let start = out.len();
        if self.push_date(st, out) {
            out.push(' ');
            if !self.push_time(st, out) {
                out.truncate(start);
            }
        }
    }

    /// Appends `st`'s short date; whether it could.
    fn push_date(&self, st: &SYSTEMTIME, out: &mut String) -> bool {
        push_formatted::<ROOM>(out, |buf, cch| {
            // SAFETY: the name is null or NUL-terminated, `st` lives through the call, and
            // `buf` is null with `cch` 0 or holds `cch` units (`push_formatted`).
            unsafe {
                GetDateFormatEx(
                    self.name_ptr(),
                    DATE_SHORTDATE,
                    st,
                    ptr::null(),
                    buf,
                    cch,
                    ptr::null(),
                )
            }
        })
    }

    /// Appends `st`'s time in the short time picture; whether it could.
    fn push_time(&self, st: &SYSTEMTIME, out: &mut String) -> bool {
        let (flags, picture) = if self.short_time.is_empty() {
            (TIME_NOSECONDS, ptr::null())
        } else {
            (0, self.short_time.as_ptr())
        };
        push_formatted::<ROOM>(out, |buf, cch| {
            // SAFETY: as in `push_date`; the picture is null or NUL-terminated, and `self`
            // holds it through the call.
            unsafe { GetTimeFormatEx(self.name_ptr(), flags, st, picture, buf, cch) }
        })
    }

    /// `utc` in the current zone, by its rules for that year: the UTC year's, so in the hours
    /// around a new year in which a zone's offset changed, it's off by the change, as Windows'
    /// own conversion documents.
    fn local_of(&self, utc: &SYSTEMTIME) -> Option<SYSTEMTIME> {
        let rules = self.rules_in(utc.wYear);
        let mut local = SYSTEMTIME::default();
        // SAFETY: a null zone is the current one, by its standing rules; all three are live.
        let ok = unsafe {
            SystemTimeToTzSpecificLocalTime(
                rules.as_ref().map_or(ptr::null(), ptr::from_ref),
                utc,
                &raw mut local,
            )
        };
        (ok != 0).then_some(local)
    }

    /// The zone's rules in `year`, read from the registry once while the years run (a table's
    /// moments are mostly in a few); `None` for its standing rules.
    fn rules_in(&self, year: u16) -> Option<TIME_ZONE_INFORMATION> {
        if self.dst_off {
            return None;
        }
        if let Some((kept, rules)) = self.rules.get()
            && kept == year
        {
            return rules;
        }
        let rules = zone_in(year);
        self.rules.set(Some((year, rules)));
        rules
    }
}

/// Appends what a `Get*FormatEx` call writes. `get(buf, cch)` makes the call into `cch` units
/// at `buf`, or returns the units the text needs, with its NUL, for a null `buf` and a `cch`
/// of 0. It writes into `N` units on the stack, and, if they're too few, into as many as it
/// asks for. Whether it wrote any.
fn push_formatted<const N: usize>(
    out: &mut String,
    mut get: impl FnMut(*mut u16, i32) -> i32,
) -> bool {
    let mut buf = [0; N];
    let n = get(buf.as_mut_ptr(), i32::try_from(N).unwrap_or(i32::MAX));
    if n > 0 {
        return push_written(&buf, n, out);
    }
    // Too few units, or the call failed: then asking for the size fails too.
    let need = get(ptr::null_mut(), 0);
    let Some(len) = usize::try_from(need).ok().filter(|&n| n > N) else {
        return false;
    };
    let mut buf = vec![0; len];
    let n = get(buf.as_mut_ptr(), need);
    push_written(&buf, n, out)
}

/// Appends what a `Get*FormatEx` call wrote: `n` units with the NUL. Whether it wrote any.
fn push_written(buf: &[u16], n: i32, out: &mut String) -> bool {
    let Some(len) = usize::try_from(n).ok().filter(|&n| n > 0 && n <= buf.len()) else {
        return false;
    };
    push_utf16(&buf[..len - 1], out);
    true
}

fn push_utf16(units: &[u16], out: &mut String) {
    out.extend(char::decode_utf16(units.iter().copied()).map(|c| c.unwrap_or('\u{FFFD}')));
}

/// `locale`'s `lctype`, in `buf`, without its NUL; `None` if it can't be read.
fn info(locale: PCWSTR, lctype: u32, buf: &mut [u16; ROOM]) -> Option<&[u16]> {
    // SAFETY: the name is null or NUL-terminated; the length is `buf`'s.
    let n = unsafe { GetLocaleInfoEx(locale, lctype, buf.as_mut_ptr(), ROOM_CCH) };
    let len = usize::try_from(n).ok().filter(|&n| n > 0 && n <= ROOM)?;
    Some(&buf[..len - 1])
}

/// The moment `t` as a `SYSTEMTIME` in UTC; `None` past what Windows can show.
fn system_time(t: UnixMillis) -> Option<SYSTEMTIME> {
    let ticks = t.0.checked_mul(10_000)?.checked_add(EPOCH_TICKS)?;
    let ft = FILETIME {
        dwLowDateTime: u32::try_from(ticks & 0xFFFF_FFFF).ok()?,
        dwHighDateTime: u32::try_from(ticks >> 32).ok()?,
    };
    let mut utc = SYSTEMTIME::default();
    // SAFETY: both point to live values of their types.
    if unsafe { FileTimeToSystemTime(&raw const ft, &raw mut utc) } == 0 {
        return None;
    }
    Some(utc)
}

/// The current zone's rules in `year`: as they were that year where Windows keeps a zone's
/// history (offsets and daylight dates that changed), else as they stand. `None` if Windows
/// can't read them; the conversion then takes the standing rules.
fn zone_in(year: u16) -> Option<TIME_ZONE_INFORMATION> {
    let mut zone = TIME_ZONE_INFORMATION::default();
    // SAFETY: a null dynamic zone is the current one; `zone` is live.
    let ok = unsafe { GetTimeZoneInformationForYear(year, ptr::null(), &raw mut zone) };
    (ok != 0).then_some(zone)
}

/// The moment a local time in the current zone is; the inverse of [`time`](Locale::time)'s
/// conversion (tests).
#[cfg(test)]
fn utc_of(local: &SYSTEMTIME) -> Option<UnixMillis> {
    use windows_sys::Win32::System::Time::{SystemTimeToFileTime, TzSpecificLocalTimeToSystemTime};
    let zone = zone_in(local.wYear)?;
    let mut utc = SYSTEMTIME::default();
    // SAFETY: all three are live.
    if unsafe { TzSpecificLocalTimeToSystemTime(&raw const zone, local, &raw mut utc) } == 0 {
        return None;
    }
    let mut ft = FILETIME::default();
    // SAFETY: both are live.
    if unsafe { SystemTimeToFileTime(&raw const utc, &raw mut ft) } == 0 {
        return None;
    }
    let ticks = u64::from(ft.dwHighDateTime) << 32 | u64::from(ft.dwLowDateTime);
    Some(UnixMillis(ticks.checked_sub(EPOCH_TICKS)? / 10_000))
}

/// A hash of what the user's formats write, and the zone: the locale's name, its short date
/// and time pictures, its AM and PM symbols, its calendar, its decimal separator, and the
/// zone's key, bias and daylight setting. Never 0, which `Env`'s default has.
#[must_use]
pub fn stamp() -> u64 {
    let mut h = Fnv::new();
    let mut buf = [0; ROOM];
    for lctype in [
        LOCALE_SNAME,
        LOCALE_SSHORTDATE,
        LOCALE_SSHORTTIME,
        LOCALE_S1159,
        LOCALE_S2359,
        LOCALE_ICALENDARTYPE,
        LOCALE_SDECIMAL,
    ] {
        if let Some(units) = info(ptr::null(), lctype, &mut buf) {
            h.units(units);
        }
        h.unit(0);
    }
    let mut zone = DYNAMIC_TIME_ZONE_INFORMATION::default();
    // SAFETY: `zone` is a live value of its type.
    let id = unsafe { GetDynamicTimeZoneInformation(&raw mut zone) };
    let key = zone.TimeZoneKeyName;
    let len = key.iter().position(|&u| u == 0).unwrap_or(key.len());
    h.units(&key[..len]);
    h.unit(0);
    for x in [
        zone.Bias.cast_unsigned(),
        zone.DaylightBias.cast_unsigned(),
        u32::from(zone.DynamicDaylightTimeDisabled),
        id,
    ] {
        h.bytes(&x.to_le_bytes());
    }
    h.0 | 1
}

/// FNV-1a, 64-bit, over UTF-16 units.
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
    }

    fn unit(&mut self, u: u16) {
        self.bytes(&u.to_le_bytes());
    }

    fn units(&mut self, units: &[u16]) {
        for &u in units {
            self.unit(u);
        }
    }
}

#[cfg(test)]
mod tests {
    use dpro_core::units::UnixMillis;

    use super::*;

    /// 2026-11-06 15:04:00 UTC.
    const NOV_6_1504: u64 = 1_793_977_440_000;
    /// 2026-11-06, the day.
    const NOV_6: u64 = 1_793_923_200_000;

    fn date(l: &Locale, ms: u64) -> String {
        let mut out = String::new();
        l.date(UnixMillis(ms), &mut out);
        out
    }

    fn utc(l: &Locale, ms: u64) -> String {
        let mut out = String::new();
        // In UTC: a test mustn't depend on the runner's zone.
        if let Some(st) = system_time(UnixMillis(ms)) {
            l.push_moment(&st, &mut out);
        }
        out
    }

    /// T2/T3: a named locale's formats, as Windows (and Wine) write them.
    #[test]
    fn en_us_reads_as_windows_writes_it() {
        let l = Locale::named("en-US").expect("en-US");
        assert_eq!(l.decimal(), ".");
        assert_eq!(date(&l, NOV_6), "11/6/2026");
        assert_eq!(utc(&l, NOV_6_1504), "11/6/2026 3:04 PM");
    }

    /// On the 16th, as a single-digit day reads "06." on Windows (`dd.MM.yyyy`) and "6." in
    /// Wine (`d.M.yyyy`).
    #[test]
    fn de_de_has_its_own_separators() {
        const TEN_DAYS: u64 = 10 * 86_400_000;
        let l = Locale::named("de-DE").expect("de-DE");
        assert_eq!(l.decimal(), ",");
        assert_eq!(date(&l, NOV_6 + TEN_DAYS), "16.11.2026");
        assert_eq!(utc(&l, NOV_6_1504 + TEN_DAYS), "16.11.2026 15:04");
    }

    /// A moment's time is in the short time picture, not the long one without its seconds.
    #[test]
    fn a_moment_reads_in_the_short_time_picture() {
        let mut l = Locale::named("en-US").expect("en-US");
        l.short_time = "HH:mm".encode_utf16().chain([0]).collect();
        assert_eq!(utc(&l, NOV_6_1504), "11/6/2026 15:04");
    }

    /// A text longer than the stack buffer is written in full, into one of the size Windows
    /// asks for.
    #[test]
    fn a_text_longer_than_the_buffer_is_written_in_full() {
        let l = Locale::named("en-US").expect("en-US");
        let st = system_time(UnixMillis(NOV_6)).expect("a day");
        let mut out = String::from("on ");
        let wrote = push_formatted::<4>(&mut out, |buf, cch| {
            // SAFETY: as in `push_date`.
            unsafe {
                GetDateFormatEx(
                    l.name_ptr(),
                    DATE_SHORTDATE,
                    &raw const st,
                    ptr::null(),
                    buf,
                    cch,
                    ptr::null(),
                )
            }
        });
        assert!(wrote);
        assert_eq!(out, "on 11/6/2026");
    }

    #[test]
    fn a_name_windows_doesn_t_know_is_none() {
        assert!(Locale::named("xx-NOWHERE").is_none());
        assert!(Locale::named("").is_none());
    }

    /// The runner's own formats: printed, for TD-0009 §13.
    #[test]
    fn the_user_s_formats_read() {
        let l = Locale::user();
        let mut moment = String::new();
        l.time(UnixMillis(NOV_6_1504), &mut moment);
        println!(
            "user formats: decimal {:?}, day {:?}, moment {:?}",
            l.decimal(),
            date(&l, NOV_6),
            moment
        );
        assert_ne!(l.decimal(), "");
        assert_ne!(date(&l, NOV_6), "");
        assert_ne!(moment, "");
    }

    /// A day is that day anywhere: no time zone moves it.
    #[test]
    fn a_day_is_the_same_day_in_every_zone() {
        let l = Locale::named("en-US").expect("en-US");
        // Its last millisecond is still the 6th.
        assert_eq!(date(&l, NOV_6 + 86_399_999), "11/6/2026");
    }

    /// The zone's rules applied to a moment and back give the moment (to the millisecond).
    #[test]
    fn a_moment_in_local_time_converts_back_to_itself() {
        let l = Locale::user();
        // Years apart, and the first again: its rules are read again.
        for ms in [0, NOV_6_1504, 1_720_000_000_123, 0] {
            let utc = system_time(UnixMillis(ms)).expect("a moment");
            let local = l.local_of(&utc).expect("a local time");
            assert_eq!(utc_of(&local), Some(UnixMillis(ms)), "{ms}");
        }
    }

    /// Past SYSTEMTIME's last year (30827), nothing is written.
    #[test]
    fn a_moment_windows_can_t_show_appends_nothing() {
        let l = Locale::named("en-US").expect("en-US");
        assert_eq!(date(&l, u64::MAX), "");
        assert_eq!(utc(&l, u64::MAX), "");
        let mut out = String::from("kept");
        l.time(UnixMillis(u64::MAX), &mut out);
        assert_eq!(out, "kept");
    }

    /// The stamp changes only when a format or the zone does.
    #[test]
    fn the_stamp_is_the_same_until_a_format_changes() {
        let a = stamp();
        println!("locale stamp {a:#018x}");
        assert_eq!(stamp(), a);
    }
}
