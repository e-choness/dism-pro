//! CRC-32C (Castagnoli), table-driven. Small enough to own instead of a dependency; the
//! journal only checks record integrity, it isn't a hot loop next to the fsync.

const POLY: u32 = 0x82F6_3B78; // reflected 0x1EDC6F41

const TABLE: [u32; 256] = {
    let mut table = [0_u32; 256];
    let mut i = 0;
    while i < 256 {
        // i < 256, so the cast is exact.
        #[allow(clippy::cast_possible_truncation)]
        let mut crc = i as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 == 1 { (crc >> 1) ^ POLY } else { crc >> 1 };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
};

pub(crate) fn crc32c(data: &[u8]) -> u32 {
    let mut crc = !0_u32;
    for &b in data {
        crc = TABLE[usize::from(crc.to_le_bytes()[0] ^ b)] ^ (crc >> 8);
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_check_value() {
        // RFC 3720 / iSCSI check value for "123456789".
        assert_eq!(crc32c(b"123456789"), 0xE306_9283);
        assert_eq!(crc32c(b""), 0);
    }
}
