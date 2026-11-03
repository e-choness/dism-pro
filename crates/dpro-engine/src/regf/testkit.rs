//! Builds valid hive images for tests and benches: one `hbin`, cells in key order.

use std::fmt::Write as _;

/// The subkey list form a key is written with. The OS picks one by hive age and key size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum List {
    Li,
    Lf,
    Lh,
    /// `ri` over `lh` lists of this many entries.
    Ri(usize),
}

#[derive(Debug, Clone)]
pub(crate) struct TestKey {
    pub(crate) name: String,
    pub(crate) values: Vec<(String, u32, Vec<u8>)>,
    pub(crate) subkeys: Vec<TestKey>,
    pub(crate) list: List,
}

impl TestKey {
    pub(crate) fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            values: Vec::new(),
            subkeys: Vec::new(),
            list: List::Lh,
        }
    }

    pub(crate) fn value(mut self, name: &str, kind: u32, data: &[u8]) -> Self {
        self.values.push((name.to_owned(), kind, data.to_vec()));
        self
    }

    pub(crate) fn subkey(mut self, key: Self) -> Self {
        self.subkeys.push(key);
        self
    }

    pub(crate) fn list(mut self, list: List) -> Self {
        self.list = list;
        self
    }
}

const BASE: usize = 0x1000;
const HBIN_HEADER: usize = 0x20;
const SEGMENT: usize = 16344;

fn fits_latin1(s: &str) -> bool {
    s.chars().all(|c| u32::from(c) < 0x100)
}

/// (name bytes, stored as Latin-1?)
fn stored(s: &str) -> (Vec<u8>, bool) {
    if fits_latin1(s) {
        (
            s.chars().filter_map(|c| u8::try_from(c).ok()).collect(),
            true,
        )
    } else {
        (s.encode_utf16().flat_map(u16::to_le_bytes).collect(), false)
    }
}

fn offset_of(buf: &[u8]) -> u32 {
    u32::try_from(buf.len()).unwrap_or(u32::MAX)
}

/// Appends a cell, 8-aligned as the format requires, and returns its offset.
fn cell(buf: &mut Vec<u8>, payload: &[u8]) -> u32 {
    let at = offset_of(buf);
    let size = (payload.len() + 4).next_multiple_of(8);
    let stored = -i32::try_from(size).unwrap_or(i32::MAX);
    buf.extend_from_slice(&stored.to_le_bytes());
    buf.extend_from_slice(payload);
    buf.resize(buf.len() + size - payload.len() - 4, 0);
    at
}

fn data_offset(buf: &mut Vec<u8>, data: &[u8]) -> (u32, u32) {
    let len = u32::try_from(data.len()).unwrap_or(u32::MAX);
    if data.len() <= 4 {
        let mut inline = [0; 4];
        inline[..data.len()].copy_from_slice(data);
        return (len | 0x8000_0000, u32::from_le_bytes(inline));
    }
    if data.len() <= SEGMENT {
        return (len, cell(buf, data));
    }
    let segments: Vec<u32> = data.chunks(SEGMENT).map(|c| cell(buf, c)).collect();
    let list: Vec<u8> = segments.iter().flat_map(|s| s.to_le_bytes()).collect();
    let list = cell(buf, &list);
    let mut db = b"db".to_vec();
    db.extend_from_slice(&u16::try_from(segments.len()).unwrap_or(0).to_le_bytes());
    db.extend_from_slice(&list.to_le_bytes());
    db.extend_from_slice(&[0; 4]);
    (len, cell(buf, &db))
}

fn value(buf: &mut Vec<u8>, (name, kind, data): &(String, u32, Vec<u8>)) -> u32 {
    let (len, offset) = data_offset(buf, data);
    let (name, latin1) = stored(name);
    let mut vk = b"vk".to_vec();
    vk.extend_from_slice(&u16::try_from(name.len()).unwrap_or(0).to_le_bytes());
    vk.extend_from_slice(&len.to_le_bytes());
    vk.extend_from_slice(&offset.to_le_bytes());
    vk.extend_from_slice(&kind.to_le_bytes());
    vk.extend_from_slice(&u16::from(latin1).to_le_bytes());
    vk.extend_from_slice(&[0; 2]);
    vk.extend_from_slice(&name);
    cell(buf, &vk)
}

fn plain_list(buf: &mut Vec<u8>, signature: [u8; 2], keys: &[u32], hints: bool) -> u32 {
    let mut list = signature.to_vec();
    list.extend_from_slice(&u16::try_from(keys.len()).unwrap_or(0).to_le_bytes());
    for key in keys {
        list.extend_from_slice(&key.to_le_bytes());
        if hints {
            list.extend_from_slice(&[0; 4]);
        }
    }
    cell(buf, &list)
}

fn subkey_list(buf: &mut Vec<u8>, keys: &[u32], kind: List) -> u32 {
    match kind {
        List::Li => plain_list(buf, *b"li", keys, false),
        List::Lf => plain_list(buf, *b"lf", keys, true),
        List::Lh => plain_list(buf, *b"lh", keys, true),
        List::Ri(size) => {
            let lists: Vec<u32> = keys
                .chunks(size.max(1))
                .map(|c| plain_list(buf, *b"lh", c, true))
                .collect();
            plain_list(buf, *b"ri", &lists, false)
        }
    }
}

fn key(buf: &mut Vec<u8>, key_: &TestKey, flags: u16) -> u32 {
    let subkeys: Vec<u32> = key_.subkeys.iter().map(|k| key(buf, k, 0)).collect();
    let values: Vec<u32> = key_.values.iter().map(|v| value(buf, v)).collect();
    let subkey_list = if subkeys.is_empty() {
        u32::MAX
    } else {
        subkey_list(buf, &subkeys, key_.list)
    };
    let value_list = if values.is_empty() {
        u32::MAX
    } else {
        let list: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        cell(buf, &list)
    };
    let (name, latin1) = stored(&key_.name);
    let mut nk = vec![0; 0x4C];
    nk[..2].copy_from_slice(b"nk");
    let flags = flags | if latin1 { 0x20 } else { 0 };
    nk[0x02..0x04].copy_from_slice(&flags.to_le_bytes());
    nk[0x04..0x0C].copy_from_slice(&0x01DC_0000_0000_0000_u64.to_le_bytes());
    let count = |n: usize| u32::try_from(n).unwrap_or(0).to_le_bytes();
    nk[0x14..0x18].copy_from_slice(&count(subkeys.len()));
    nk[0x1C..0x20].copy_from_slice(&subkey_list.to_le_bytes());
    nk[0x24..0x28].copy_from_slice(&count(values.len()));
    nk[0x28..0x2C].copy_from_slice(&value_list.to_le_bytes());
    nk[0x48..0x4A].copy_from_slice(&u16::try_from(name.len()).unwrap_or(0).to_le_bytes());
    nk.extend_from_slice(&name);
    cell(buf, &nk)
}

/// The hive image of `root`.
pub(crate) fn build(root: &TestKey) -> Vec<u8> {
    let mut bins = vec![0; HBIN_HEADER];
    let root = key(&mut bins, root, 0x4 | 0x8);
    // The bin ends on a page: the rest is one free cell.
    let end = bins.len().next_multiple_of(BASE).max(BASE);
    let free = i32::try_from(end - bins.len()).unwrap_or(0);
    if free >= 4 {
        bins.extend_from_slice(&free.to_le_bytes());
    }
    bins.resize(end, 0);
    let size = u32::try_from(end).unwrap_or(0);
    bins[..4].copy_from_slice(b"hbin");
    bins[0x08..0x0C].copy_from_slice(&size.to_le_bytes());

    let mut base = vec![0; BASE];
    base[..4].copy_from_slice(b"regf");
    base[0x04..0x08].copy_from_slice(&1_u32.to_le_bytes());
    base[0x08..0x0C].copy_from_slice(&1_u32.to_le_bytes());
    base[0x14..0x18].copy_from_slice(&1_u32.to_le_bytes());
    base[0x18..0x1C].copy_from_slice(&5_u32.to_le_bytes());
    base[0x20..0x24].copy_from_slice(&1_u32.to_le_bytes());
    base[0x24..0x28].copy_from_slice(&root.to_le_bytes());
    base[0x28..0x2C].copy_from_slice(&size.to_le_bytes());
    seal(&mut base);
    base.extend_from_slice(&bins);
    base
}

/// Recomputes the base block checksum after an edit.
pub(crate) fn seal(image: &mut [u8]) {
    let sum = image[..0x1FC]
        .as_chunks::<4>()
        .0
        .iter()
        .fold(0_u32, |sum, d| sum ^ u32::from_le_bytes(*d));
    let sum = match sum {
        u32::MAX => u32::MAX - 1,
        0 => 1,
        sum => sum,
    };
    image[0x1FC..0x200].copy_from_slice(&sum.to_le_bytes());
}

/// A machine-sized hive: `HKLM\SOFTWARE`-shaped keys up to about `bytes`, with the
/// `Microsoft\Windows NT\CurrentVersion` key the engine reads.
pub(crate) fn software_like(bytes: usize) -> Vec<u8> {
    let current = TestKey::new("CurrentVersion").value(
        "CurrentBuild",
        1,
        &"26100\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    let microsoft = TestKey::new("Microsoft").subkey(TestKey::new("Windows NT").subkey(current));
    let mut root = TestKey::new("ROOT").subkey(microsoft);
    let mut name = String::new();
    // ~1 KiB per leaf key once its 4 values and their cells are in.
    let vendors = 200;
    let leaves = (bytes / 1100 / vendors).max(1);
    for v in 0..vendors {
        let mut vendor = TestKey::new(&format!("Vendor{v:04}")).list(List::Ri(256));
        for l in 0..leaves {
            name.clear();
            let _ = write!(name, "Key{l:06}");
            let mut leaf = TestKey::new(&name);
            for i in 0..4 {
                leaf = leaf.value(&format!("Value{i}"), 1, &[b'x'; 180]);
            }
            vendor = vendor.subkey(leaf);
        }
        root = root.subkey(vendor);
    }
    build(&root)
}
