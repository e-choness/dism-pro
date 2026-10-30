//! Human byte sizes with Windows Explorer conventions (1 KB = 1024 B, one decimal), and UTC
//! timestamps.

use dpro_core::units::{Bytes, UnixMillis};

use crate::messages::Messages;

/// `4.2 GB`, `512 B`, using the catalog's unit labels and decimal separator.
#[must_use]
pub fn format_bytes(bytes: Bytes, m: &Messages) -> String {
    const UNITS: [&str; 4] = ["unit-kb", "unit-mb", "unit-gb", "unit-tb"];
    let mut value = bytes.0;
    if value < 1024 {
        return m.get("unit-bytes", &[("value", &value)]);
    }
    // Tenths of the unit, computed in integers (no float formatting differences).
    let mut unit = 0;
    let mut divisor: u128 = 1024;
    while unit + 1 < UNITS.len() && u128::from(value) >= divisor * 1024 {
        divisor *= 1024;
        unit += 1;
    }
    let tenths = (u128::from(value) * 10 + divisor / 2) / divisor;
    value = u64::try_from(tenths).unwrap_or(u64::MAX);
    let number = format!(
        "{}{}{}",
        value / 10,
        m.text("fmt-decimal-separator"),
        value % 10
    );
    m.get(UNITS[unit], &[("value", &number)])
}

/// `2026-10-30 14:03 UTC` (minutes), for undo deadlines and job lists.
#[must_use]
pub fn format_utc(t: UnixMillis, m: &Messages) -> String {
    let c = Civil::from(t);
    let date = format!("{:04}-{:02}-{:02}", c.year, c.month, c.day);
    let time = format!("{:02}:{:02}", c.hour, c.minute);
    m.get("fmt-utc", &[("date", &date), ("time", &time)])
}

/// RFC 3339 in UTC with whole seconds (`2026-10-30T14:03:05Z`), for JSON.
#[must_use]
pub fn rfc3339(t: UnixMillis) -> String {
    let c = Civil::from(t);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        c.year, c.month, c.day, c.hour, c.minute, c.second
    )
}

/// Proleptic Gregorian date and time of day in UTC.
struct Civil {
    year: u64,
    month: u64,
    day: u64,
    hour: u64,
    minute: u64,
    second: u64,
}

impl From<UnixMillis> for Civil {
    /// Days to date: H. Hinnant, "chrono-Compatible Low-Level Date Algorithms"
    /// (`civil_from_days`), restricted to dates from 1970 on, so all arithmetic is unsigned.
    fn from(t: UnixMillis) -> Self {
        let secs = t.0 / 1000;
        let (days, rem) = (secs / 86_400, secs % 86_400);
        let z = days + 719_468;
        let era = z / 146_097;
        let doe = z % 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        Self {
            year: yoe + era * 400 + u64::from(month <= 2),
            month,
            day: doy - (153 * mp + 2) / 5 + 1,
            hour: rem / 3600,
            minute: rem % 3600 / 60,
            second: rem % 60,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_dates() {
        let m = Messages::en_us();
        assert_eq!(rfc3339(UnixMillis(0)), "1970-01-01T00:00:00Z");
        // 2000-02-29 (leap day of a century leap year) 23:59:59.999
        assert_eq!(rfc3339(UnixMillis(951_868_799_999)), "2000-02-29T23:59:59Z");
        assert_eq!(rfc3339(UnixMillis(951_868_800_000)), "2000-03-01T00:00:00Z");
        // 2100 is not a leap year.
        assert_eq!(
            rfc3339(UnixMillis(4_107_542_400_000)),
            "2100-03-01T00:00:00Z"
        );
        assert_eq!(
            format_utc(UnixMillis(1_793_365_380_000), &m),
            "2026-10-30 13:03 UTC"
        );
    }

    #[test]
    fn formats_like_explorer() {
        let m = Messages::en_us();
        assert_eq!(format_bytes(Bytes(0), &m), "0 B");
        assert_eq!(format_bytes(Bytes(1023), &m), "1023 B");
        assert_eq!(format_bytes(Bytes(1024), &m), "1.0 KB");
        assert_eq!(format_bytes(Bytes(1536), &m), "1.5 KB");
        assert_eq!(format_bytes(Bytes(4_509_715_660), &m), "4.2 GB");
        assert_eq!(format_bytes(Bytes(u64::MAX), &m), "16777216.0 TB");
    }
}
