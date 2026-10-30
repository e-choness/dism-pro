//! NTFS file record segments → [`Volume`]. Layout from the public NTFS on-disk format
//! documentation: the `FILE` record header, multi-sector fixups (512-byte stride), attribute
//! headers, `$FILE_NAME`, and mapping pairs (run lists). Pure: no OS calls, tested on Linux.

use crate::tally::{RECORD_MASK, Volume};

const ATTRIBUTE_LIST: u32 = 0x20;
const FILE_NAME: u32 = 0x30;
const DATA: u32 = 0x80;
const END: u32 = 0xFFFF_FFFF;
const IN_USE: u16 = 0x0001;
const DIRECTORY: u16 = 0x0002;
/// 8.3 alias of a long name; the long name has its own `$FILE_NAME`.
const DOS_NAMESPACE: u8 = 2;
const FIXUP_STRIDE: usize = 512;

/// Why a record segment added nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    /// Never used: all zeros.
    Empty,
    /// A deleted file's record.
    Free,
    BadSignature,
    /// A sector's fixup didn't match: the record was being written.
    Torn,
    Malformed,
}

fn le<const N: usize>(b: &[u8], at: usize) -> Result<[u8; N], Skip> {
    b.get(at..at + N)
        .and_then(|s| s.try_into().ok())
        .ok_or(Skip::Malformed)
}
fn u16_at(b: &[u8], at: usize) -> Result<u16, Skip> {
    le(b, at).map(u16::from_le_bytes)
}
fn u32_at(b: &[u8], at: usize) -> Result<u32, Skip> {
    le(b, at).map(u32::from_le_bytes)
}
fn u64_at(b: &[u8], at: usize) -> Result<u64, Skip> {
    le(b, at).map(u64::from_le_bytes)
}

/// Undoes the multi-sector protection: on disk the last two bytes of every 512-byte stride hold
/// the update sequence number, and the real bytes live in the update sequence array.
pub fn apply_fixups(rec: &mut [u8]) -> Result<(), Skip> {
    let at = usize::from(u16_at(rec, 4)?);
    let count = usize::from(u16_at(rec, 6)?);
    if count < 2 || (count - 1) * FIXUP_STRIDE > rec.len() || at + 2 * count > rec.len() {
        return Err(Skip::Malformed);
    }
    let usn = [rec[at], rec[at + 1]];
    for i in 1..count {
        let end = i * FIXUP_STRIDE;
        if rec[end - 2..end] != usn {
            return Err(Skip::Torn);
        }
        rec[end - 2] = rec[at + 2 * i];
        rec[end - 1] = rec[at + 2 * i + 1];
    }
    Ok(())
}

/// Attributes of a fixed-up record: (type, bytes of the whole attribute).
fn attributes(rec: &[u8]) -> impl Iterator<Item = Result<(u32, &[u8]), Skip>> {
    let used = u32_at(rec, 0x18).map_or(0, |u| (u as usize).min(rec.len()));
    let mut at = u16_at(rec, 0x14).map_or(usize::MAX, usize::from);
    std::iter::from_fn(move || {
        if at.saturating_add(8) > used {
            return None;
        }
        let kind = match u32_at(rec, at) {
            Ok(END) => return None,
            Ok(kind) => kind,
            Err(e) => return Some(Err(e)),
        };
        let len = u32_at(rec, at + 4).map_or(0, |l| l as usize);
        if len < 0x18 || at + len > used {
            at = usize::MAX;
            return Some(Err(Skip::Malformed));
        }
        let attr = &rec[at..at + len];
        at += len;
        Some(Ok((kind, attr)))
    })
}

fn resident_value(attr: &[u8]) -> Result<&[u8], Skip> {
    let len = u32_at(attr, 0x10)? as usize;
    let at = usize::from(u16_at(attr, 0x14)?);
    attr.get(at..at + len).ok_or(Skip::Malformed)
}

fn is_named(attr: &[u8]) -> bool {
    attr[9] != 0
}

fn is_resident(attr: &[u8]) -> bool {
    attr[8] == 0
}

/// Adds record segment number `number` (fixups not yet applied) to `vol`. Extension segments
/// (`base` ≠ 0) carry attributes of their base record and are merged into it.
pub fn add_record(vol: &mut Volume, number: u64, rec: &mut [u8]) -> Result<(), Skip> {
    match rec.get(..4) {
        Some(b"FILE") => {}
        Some([0, 0, 0, 0]) => return Err(Skip::Empty),
        _ => return Err(Skip::BadSignature),
    }
    apply_fixups(rec)?;
    let flags = u16_at(rec, 0x16)?;
    if flags & IN_USE == 0 {
        return Err(Skip::Free);
    }
    let base = u64_at(rec, 0x20)? & RECORD_MASK;
    let owner = if base == 0 { number } else { base };
    if base == 0 {
        let r = vol.record_mut(owner);
        r.in_use = true;
        r.is_dir = flags & DIRECTORY != 0;
    }
    for attr in attributes(rec) {
        let (kind, attr) = attr?;
        match kind {
            FILE_NAME => {
                let value = resident_value(attr)?;
                if value.len() < 0x42 {
                    return Err(Skip::Malformed);
                }
                if value[0x41] == DOS_NAMESPACE {
                    continue;
                }
                let parent = u64_at(value, 0)? & RECORD_MASK;
                let chars = usize::from(value[0x40]);
                let text = value.get(0x42..0x42 + 2 * chars).ok_or(Skip::Malformed)?;
                let units = text
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| u16::from_le_bytes([c[0], c[1]]));
                vol.push_name(owner, parent, units);
            }
            // Only the unnamed stream is the file's size; named ones are alternate data streams.
            DATA if !is_named(attr) => {
                let size = if is_resident(attr) {
                    resident_value(attr)?.len() as u64
                } else if u64_at(attr, 0x10)? == 0 {
                    u64_at(attr, 0x30)?
                } else {
                    continue; // a later extent of a split attribute: the first one has the size
                };
                vol.record_mut(owner).size = size;
            }
            _ => {}
        }
    }
    Ok(())
}

/// One run of a non-resident attribute: `clusters` clusters at `lcn` (`None`: sparse).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run {
    pub lcn: Option<u64>,
    pub clusters: u64,
}

/// Decodes mapping pairs: a header byte (low nibble: length bytes, high nibble: offset bytes),
/// then the run length and a signed LCN delta from the previous run, until a zero byte.
pub fn decode_runs(mut bytes: &[u8]) -> Result<Vec<Run>, Skip> {
    let mut runs = Vec::new();
    let mut lcn: i64 = 0;
    loop {
        let (&head, rest) = bytes.split_first().ok_or(Skip::Malformed)?;
        if head == 0 {
            return Ok(runs);
        }
        let (len_size, off_size) = (usize::from(head & 0x0F), usize::from(head >> 4));
        if len_size == 0 || len_size > 8 || off_size > 8 || rest.len() < len_size + off_size {
            return Err(Skip::Malformed);
        }
        let clusters = unsigned(&rest[..len_size]);
        if off_size == 0 {
            runs.push(Run {
                lcn: None,
                clusters,
            });
        } else {
            lcn = lcn
                .checked_add(signed(&rest[len_size..len_size + off_size]))
                .filter(|l| *l >= 0)
                .ok_or(Skip::Malformed)?;
            runs.push(Run {
                lcn: Some(lcn.cast_unsigned()),
                clusters,
            });
        }
        bytes = &rest[len_size + off_size..];
    }
}

fn unsigned(b: &[u8]) -> u64 {
    b.iter().rev().fold(0, |v, &x| (v << 8) | u64::from(x))
}

fn signed(b: &[u8]) -> i64 {
    let shift = 64 - 8 * b.len() as u32;
    (unsigned(b) << shift).cast_signed() >> shift
}

/// Where the MFT itself lives: the runs of record 0's unnamed `$DATA` (record 0 is `$MFT`).
pub fn mft_runs(rec0: &mut [u8]) -> Result<Vec<Run>, String> {
    apply_fixups(rec0).map_err(|e| format!("$MFT record: {e:?}"))?;
    let mut runs = None;
    for attr in attributes(rec0) {
        let (kind, attr) = attr.map_err(|e| format!("$MFT record: {e:?}"))?;
        if kind == ATTRIBUTE_LIST {
            return Err("$MFT has an attribute list (very fragmented MFT): not handled".into());
        }
        if kind == DATA && !is_named(attr) && !is_resident(attr) {
            let at = usize::from(u16_at(attr, 0x20).map_err(|e| format!("{e:?}"))?);
            runs = Some(decode_runs(&attr[at.min(attr.len())..]).map_err(|e| format!("{e:?}"))?);
        }
    }
    runs.ok_or_else(|| "$MFT record has no non-resident $DATA".into())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    const SIZE: usize = 1024;
    const USA_AT: usize = 0x30;

    pub(crate) fn resident(kind: u32, name: &str, value: &[u8]) -> Vec<u8> {
        let name_at = 0x18;
        let value_at = (name_at + 2 * name.len()).next_multiple_of(8);
        let mut a = vec![0; (value_at + value.len()).next_multiple_of(8)];
        a[0..4].copy_from_slice(&kind.to_le_bytes());
        let len = a.len() as u32;
        a[4..8].copy_from_slice(&len.to_le_bytes());
        a[9] = name.len() as u8;
        a[0x0A..0x0C].copy_from_slice(&(name_at as u16).to_le_bytes());
        a[0x10..0x14].copy_from_slice(&(value.len() as u32).to_le_bytes());
        a[0x14..0x16].copy_from_slice(&(value_at as u16).to_le_bytes());
        for (i, u) in name.encode_utf16().enumerate() {
            a[name_at + 2 * i..name_at + 2 * i + 2].copy_from_slice(&u.to_le_bytes());
        }
        a[value_at..value_at + value.len()].copy_from_slice(value);
        a
    }

    pub(crate) fn non_resident_data(start_vcn: u64, size: u64, runs: &[u8]) -> Vec<u8> {
        let runs_at = 0x40;
        let mut a = vec![0; (runs_at + runs.len()).next_multiple_of(8)];
        a[0..4].copy_from_slice(&DATA.to_le_bytes());
        let len = a.len() as u32;
        a[4..8].copy_from_slice(&len.to_le_bytes());
        a[8] = 1;
        a[0x10..0x18].copy_from_slice(&start_vcn.to_le_bytes());
        a[0x20..0x22].copy_from_slice(&(runs_at as u16).to_le_bytes());
        a[0x30..0x38].copy_from_slice(&size.to_le_bytes());
        a[runs_at..runs_at + runs.len()].copy_from_slice(runs);
        a
    }

    pub(crate) fn file_name(parent: u64, name: &str, namespace: u8) -> Vec<u8> {
        let units: Vec<u16> = name.encode_utf16().collect();
        let mut v = vec![0; 0x42 + 2 * units.len()];
        v[0..8].copy_from_slice(&(parent | (1 << 48)).to_le_bytes()); // sequence 1 in the top bits
        v[0x40] = units.len() as u8;
        v[0x41] = namespace;
        for (i, u) in units.iter().enumerate() {
            v[0x42 + 2 * i..0x44 + 2 * i].copy_from_slice(&u.to_le_bytes());
        }
        resident(FILE_NAME, "", &v)
    }

    /// A record as it sits on disk: fixups applied (sector ends hold the update sequence number).
    pub(crate) fn record(flags: u16, base: u64, attrs: &[Vec<u8>]) -> Vec<u8> {
        let mut r = vec![0; SIZE];
        r[0..4].copy_from_slice(b"FILE");
        r[4..6].copy_from_slice(&(USA_AT as u16).to_le_bytes());
        let count = SIZE / FIXUP_STRIDE + 1;
        r[6..8].copy_from_slice(&(count as u16).to_le_bytes());
        let first = (USA_AT + 2 * count).next_multiple_of(8);
        r[0x14..0x16].copy_from_slice(&(first as u16).to_le_bytes());
        r[0x16..0x18].copy_from_slice(&flags.to_le_bytes());
        r[0x20..0x28].copy_from_slice(&base.to_le_bytes());
        let mut at = first;
        for a in attrs {
            r[at..at + a.len()].copy_from_slice(a);
            at += a.len();
        }
        r[at..at + 4].copy_from_slice(&END.to_le_bytes());
        r[0x18..0x1C].copy_from_slice(&((at + 8) as u32).to_le_bytes());
        let usn = [0x34, 0x12];
        r[USA_AT..USA_AT + 2].copy_from_slice(&usn);
        for i in 1..count {
            let end = i * FIXUP_STRIDE;
            let saved = [r[end - 2], r[end - 1]];
            r[USA_AT + 2 * i..USA_AT + 2 * i + 2].copy_from_slice(&saved);
            r[end - 2..end].copy_from_slice(&usn);
        }
        r
    }

    #[test]
    fn fixups_restore_sector_ends_and_detect_torn_writes() {
        let attr = resident(0x10, "", &[0xAB; 600]); // spans the first sector end
        let mut r = record(IN_USE, 0, &[attr]);
        assert_eq!(&r[0x1FE..0x200], &[0x34, 0x12]);
        let mut torn = r.clone();
        apply_fixups(&mut r).unwrap();
        assert_ne!(&r[0x1FE..0x200], &[0x34, 0x12]);
        torn[0x3FF] ^= 1;
        assert_eq!(apply_fixups(&mut torn), Err(Skip::Torn));
    }

    #[test]
    fn a_file_record_gives_its_long_names_and_unnamed_data_size() {
        let mut vol = Volume::default();
        let mut r = record(
            IN_USE,
            0,
            &[
                file_name(77, "LONGFI~1.DLL", DOS_NAMESPACE),
                file_name(77, "long file name.dll", 1),
                file_name(78, "hard link.dll", 1),
                resident(DATA, "Zone.Identifier", &[1; 26]),
                non_resident_data(0, 123_456, &[0x11, 0x10, 0x20, 0]),
            ],
        );
        add_record(&mut vol, 300, &mut r).unwrap();
        assert_eq!(vol.records[300].size, 123_456);
        assert!(vol.records[300].in_use && !vol.records[300].is_dir);
        let parents: Vec<u64> = vol.names.iter().map(|n| n.parent).collect();
        assert_eq!(parents, [77, 78]);
        let first = vol.names[0];
        let text =
            String::from_utf16(&vol.arena[first.start..first.start + usize::from(first.len)]);
        assert_eq!(text.unwrap(), "long file name.dll");
    }

    #[test]
    fn extension_records_merge_into_their_base() {
        let mut vol = Volume::default();
        let mut base = record(IN_USE | DIRECTORY, 0, &[file_name(5, "dir", 1)]);
        let mut ext = record(IN_USE, 0x190 | (3 << 48), &[file_name(5, "second", 1)]);
        let mut tail = record(IN_USE, 400, &[non_resident_data(16, 999, &[0x11, 1, 1, 0])]);
        add_record(&mut vol, 401, &mut ext).unwrap();
        add_record(&mut vol, 400, &mut base).unwrap();
        add_record(&mut vol, 402, &mut tail).unwrap();
        assert!(vol.records[400].is_dir);
        assert_eq!(
            vol.records[400].size, 0,
            "only the first extent carries the size"
        );
        assert!(vol.names.iter().all(|n| n.record == 400));
        assert!(vol.records.get(401).is_none_or(|r| !r.in_use));
    }

    #[test]
    fn free_empty_and_garbage_records_are_skipped() {
        let mut vol = Volume::default();
        assert_eq!(add_record(&mut vol, 1, &mut [0; SIZE]), Err(Skip::Empty));
        assert_eq!(
            add_record(&mut vol, 1, &mut [0xFF; SIZE]),
            Err(Skip::BadSignature)
        );
        let mut free = record(0, 0, &[file_name(5, "gone", 1)]);
        assert_eq!(add_record(&mut vol, 1, &mut free), Err(Skip::Free));
        let mut bad = record(IN_USE, 0, &[file_name(5, "x", 1)]);
        let first = usize::from(u16::from_le_bytes([bad[0x14], bad[0x15]]));
        bad[first + 4..first + 8].copy_from_slice(&5000u32.to_le_bytes());
        assert_eq!(add_record(&mut vol, 2, &mut bad), Err(Skip::Malformed));
        assert!(vol.names.is_empty());
    }

    #[test]
    fn run_lists_decode_forward_backward_and_sparse_runs() {
        // 24 clusters at 0x5634; 0x30 at +0x60; 0x10 at -0x10 (0xF0); 5 sparse; end.
        let bytes = [
            0x21, 0x18, 0x34, 0x56, 0x11, 0x30, 0x60, 0x11, 0x10, 0xF0, 0x01, 0x05, 0x00,
        ];
        let runs = decode_runs(&bytes).unwrap();
        let expect = [
            (Some(0x5634), 0x18),
            (Some(0x5694), 0x30),
            (Some(0x5684), 0x10),
            (None, 5),
        ];
        let got: Vec<_> = runs.iter().map(|r| (r.lcn, r.clusters)).collect();
        assert_eq!(got, expect);
        assert_eq!(decode_runs(&[0x21, 0x18]), Err(Skip::Malformed));
        assert_eq!(decode_runs(&[0x11, 0x01, 0x80, 0x00]), Err(Skip::Malformed)); // LCN < 0
    }

    #[test]
    fn mft_runs_come_from_record_zero() {
        let mut r0 = record(
            IN_USE,
            0,
            &[
                file_name(5, "$MFT", 3),
                non_resident_data(0, 1 << 20, &[0x31, 0x40, 0x00, 0x00, 0x0C, 0x00]),
            ],
        );
        let runs = mft_runs(&mut r0).unwrap();
        assert_eq!(
            runs,
            [Run {
                lcn: Some(0x0C_0000),
                clusters: 0x40
            }]
        );
        let mut listed = record(IN_USE, 0, &[resident(ATTRIBUTE_LIST, "", &[0; 32])]);
        assert!(
            mft_runs(&mut listed)
                .unwrap_err()
                .contains("attribute list")
        );
    }
}
