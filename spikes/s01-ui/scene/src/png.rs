//! A dependency-free PNG writer (RGBA8, stored deflate blocks) for the snapshot artifacts.

use std::io::{self, Write};

/// Writes `rgba` (`w × h` straight-alpha RGBA8, rows top to bottom) as a PNG.
pub fn write(out: &mut impl Write, w: u32, h: u32, rgba: &[u8]) -> io::Result<()> {
    let row = w as usize * 4;
    if rgba.len() != row * h as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "buffer size isn't w × h × 4",
        ));
    }
    out.write_all(b"\x89PNG\r\n\x1a\n")?;
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA, deflate, adaptive filter, no interlace
    chunk(out, *b"IHDR", &ihdr)?;

    // Scanlines with filter byte 0, wrapped in zlib with stored (uncompressed) blocks.
    let mut raw = Vec::with_capacity((row + 1) * h as usize);
    for line in rgba.chunks_exact(row) {
        raw.push(0);
        raw.extend_from_slice(line);
    }
    let mut z = Vec::with_capacity(raw.len() + raw.len() / 65_535 * 5 + 16);
    z.extend_from_slice(&[0x78, 0x01]);
    let mut blocks = raw.chunks(65_535).peekable();
    while let Some(b) = blocks.next() {
        z.push(u8::from(blocks.peek().is_none()));
        let len = b.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(b);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());
    chunk(out, *b"IDAT", &z)?;
    chunk(out, *b"IEND", &[])
}

fn chunk(out: &mut impl Write, kind: [u8; 4], data: &[u8]) -> io::Result<()> {
    let len = u32::try_from(data.len()).map_err(|_| io::Error::other("PNG chunk over 4 GiB"))?;
    out.write_all(&len.to_be_bytes())?;
    out.write_all(&kind)?;
    out.write_all(data)?;
    let crc = crc32(crc32(!0, &kind), data);
    out.write_all(&(!crc).to_be_bytes())
}

fn crc32(mut crc: u32, data: &[u8]) -> u32 {
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1_u32, 0_u32);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += u32::from(x);
            b += a;
        }
        a %= 65_521;
        b %= 65_521;
    }
    (b << 16) | a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksums_match_known_values() {
        assert_eq!(!crc32(!0, b"123456789"), 0xCBF4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn writes_a_well_formed_file() {
        let mut out = Vec::new();
        let px = [255_u8, 0, 0, 255].repeat(3 * 2);
        assert!(write(&mut out, 3, 2, &px).is_ok());
        assert!(out.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(out.ends_with(&[0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82]));
        assert!(write(&mut Vec::new(), 3, 3, &px).is_err());
    }
}
