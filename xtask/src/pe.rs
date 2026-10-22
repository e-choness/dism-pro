//! Minimal PE reader: the DLL names in the import and delay-import tables (doc 04 §4).
//! Parses untrusted bytes, so every read is bounds-checked and errors instead of panicking.

/// One DLL an executable imports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Import {
    pub(crate) dll: String,
    pub(crate) delay_loaded: bool,
}

const IMPORT_DIR: usize = 1;
const DELAY_IMPORT_DIR: usize = 13;
const IMPORT_DESCRIPTOR_LEN: usize = 20;
const DELAY_DESCRIPTOR_LEN: usize = 32;
/// Guards against a malformed table without a terminator.
const MAX_DESCRIPTORS: usize = 4096;

#[derive(Debug)]
struct Section {
    virtual_address: u32,
    virtual_size: u32,
    raw_offset: u32,
    raw_size: u32,
}

pub(crate) fn imports(bytes: &[u8]) -> Result<Vec<Import>, String> {
    if bytes.get(..2) != Some(b"MZ") {
        return Err("not a PE file (no MZ header)".into());
    }
    let pe = usize_at(read_u32(bytes, 0x3c)?)?;
    if bytes.get(pe..pe + 4) != Some(b"PE\0\0") {
        return Err("missing PE signature".into());
    }
    let coff = pe + 4;
    let section_count = usize::from(read_u16(bytes, coff + 2)?);
    let optional = coff + 20;
    let optional_len = usize::from(read_u16(bytes, coff + 16)?);
    let dirs = match read_u16(bytes, optional)? {
        0x10b => optional + 96,  // PE32
        0x20b => optional + 112, // PE32+
        magic => return Err(format!("unknown optional header magic {magic:#x}")),
    };
    let dir_count = usize_at(read_u32(bytes, dirs - 4)?)?;

    let sections_at = optional + optional_len;
    let mut sections = Vec::with_capacity(section_count);
    for i in 0..section_count {
        let s = sections_at + i * 40;
        sections.push(Section {
            virtual_size: read_u32(bytes, s + 8)?,
            virtual_address: read_u32(bytes, s + 12)?,
            raw_size: read_u32(bytes, s + 16)?,
            raw_offset: read_u32(bytes, s + 20)?,
        });
    }
    let file = File { bytes, sections };

    let mut out = Vec::new();
    for (dir, delay_loaded) in [(IMPORT_DIR, false), (DELAY_IMPORT_DIR, true)] {
        if dir >= dir_count {
            continue;
        }
        let table_rva = read_u32(bytes, dirs + dir * 8)?;
        if table_rva == 0 {
            continue;
        }
        let (len, name_field) = if delay_loaded {
            (DELAY_DESCRIPTOR_LEN, 4)
        } else {
            (IMPORT_DESCRIPTOR_LEN, 12)
        };
        let table = file.offset(table_rva)?;
        for i in 0..MAX_DESCRIPTORS {
            let descriptor = table + i * len;
            let name_rva = read_u32(bytes, descriptor + name_field)?;
            if name_rva == 0 {
                break;
            }
            let dll = file.c_string(name_rva)?;
            out.push(Import { dll, delay_loaded });
        }
    }
    Ok(out)
}

#[derive(Debug)]
struct File<'a> {
    bytes: &'a [u8],
    sections: Vec<Section>,
}

impl File<'_> {
    fn offset(&self, rva: u32) -> Result<usize, String> {
        let section = self.sections.iter().find(|s| {
            let size = s.virtual_size.max(s.raw_size);
            rva >= s.virtual_address && rva - s.virtual_address < size
        });
        let Some(s) = section else {
            return Err(format!("RVA {rva:#x} is outside every section"));
        };
        let delta = rva - s.virtual_address;
        if delta >= s.raw_size {
            return Err(format!("RVA {rva:#x} has no file data"));
        }
        usize_at(s.raw_offset.saturating_add(delta))
    }

    fn c_string(&self, rva: u32) -> Result<String, String> {
        let start = self.offset(rva)?;
        let rest = self.bytes.get(start..).ok_or("name past end of file")?;
        let len = rest.iter().position(|&b| b == 0).ok_or("unterminated name")?;
        let name = &rest[..len];
        if !name.is_ascii() {
            return Err(format!("non-ASCII DLL name at RVA {rva:#x}"));
        }
        Ok(String::from_utf8_lossy(name).into_owned())
    }
}

fn read_u16(bytes: &[u8], at: usize) -> Result<u16, String> {
    let b = bytes.get(at..at + 2).ok_or_else(|| truncated(at))?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}

fn read_u32(bytes: &[u8], at: usize) -> Result<u32, String> {
    let b = bytes.get(at..at + 4).ok_or_else(|| truncated(at))?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn usize_at(v: u32) -> Result<usize, String> {
    usize::try_from(v).map_err(|_| format!("offset {v:#x} does not fit in usize"))
}

fn truncated(at: usize) -> String {
    format!("file truncated at offset {at:#x}")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn put_u16(b: &mut [u8], at: usize, v: u16) {
        b[at..at + 2].copy_from_slice(&v.to_le_bytes());
    }

    fn put_u32(b: &mut [u8], at: usize, v: u32) {
        b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }

    /// A PE32+ image with one section (RVA 0x1000 ↔ file offset 0x200) holding an import
    /// table for `imports` and a delay-import table for `delayed`.
    pub(crate) fn synthetic_pe(imports: &[&str], delayed: &[&str]) -> Vec<u8> {
        let mut b = vec![0_u8; 0x1000];
        b[..2].copy_from_slice(b"MZ");
        put_u32(&mut b, 0x3c, 0x80);
        b[0x80..0x84].copy_from_slice(b"PE\0\0");
        let coff = 0x84;
        put_u16(&mut b, coff + 2, 1); // one section
        put_u16(&mut b, coff + 16, 240); // PE32+ optional header size
        let optional = coff + 20;
        put_u16(&mut b, optional, 0x20b);
        let dirs = optional + 112;
        put_u32(&mut b, dirs - 4, 16);
        let section = optional + 240;
        put_u32(&mut b, section + 8, 0xe00); // virtual size
        put_u32(&mut b, section + 12, 0x1000); // virtual address
        put_u32(&mut b, section + 16, 0xe00); // raw size
        put_u32(&mut b, section + 20, 0x200); // raw offset

        let rva = |off: usize| u32::try_from(off - 0x200 + 0x1000).unwrap();
        let mut name_at = 0x800;
        let mut table = 0x200;
        for (dir, list, len, field) in [(1, imports, 20, 12), (13, delayed, 32, 4)] {
            if list.is_empty() {
                continue;
            }
            put_u32(&mut b, dirs + dir * 8, rva(table));
            for name in list {
                b[name_at..name_at + name.len()].copy_from_slice(name.as_bytes());
                put_u32(&mut b, table + field, rva(name_at));
                name_at += name.len() + 1;
                table += len;
            }
            table += len; // zeroed terminator
        }
        b
    }

    #[test]
    fn reads_imports_and_delay_imports() {
        let pe = synthetic_pe(&["KERNEL32.dll", "ntdll.dll"], &["d3d11.dll"]);
        let found = imports(&pe).unwrap();
        let names: Vec<(&str, bool)> =
            found.iter().map(|i| (i.dll.as_str(), i.delay_loaded)).collect();
        let expected = [
            ("KERNEL32.dll", false),
            ("ntdll.dll", false),
            ("d3d11.dll", true),
        ];
        assert_eq!(names, expected);
    }

    #[test]
    fn rejects_non_pe_and_truncated_input() {
        assert!(imports(b"hello").is_err());
        let pe = synthetic_pe(&["kernel32.dll"], &[]);
        assert!(imports(&pe[..0x100]).is_err());
    }

    #[test]
    fn bad_rva_is_an_error_not_a_panic() {
        let mut pe = synthetic_pe(&["kernel32.dll"], &[]);
        put_u32(&mut pe, 0x200 + 12, 0xdead_beef);
        assert!(imports(&pe).is_err());
    }
}
