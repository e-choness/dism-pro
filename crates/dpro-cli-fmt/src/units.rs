//! Human byte sizes with Windows Explorer conventions (1 KB = 1024 B, one decimal).

use dpro_core::units::Bytes;

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
    let number = format!("{}{}{}", value / 10, m.text("fmt-decimal-separator"), value % 10);
    m.get(UNITS[unit], &[("value", &number)])
}

#[cfg(test)]
mod tests {
    use super::*;

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
