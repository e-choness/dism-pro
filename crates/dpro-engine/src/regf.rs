//! Read-only reader for registry hive files (`regf`), ADR-0008: the OS refuses to load real
//! system hives as app hives, and every other way of loading one writes to it or mounts it.
//!
//! A hive is untrusted input read by an elevated process: every offset is checked, and a bad one
//! is an error, never a panic. Offsets may also form cycles, so a walker over [`Key::subkeys`]
//! must bound its depth (the OS allows 512 levels).
//!
//! There's no log replay: a hive that wasn't closed cleanly ([`Regf::is_dirty`]) reads as of its
//! last clean state.

use std::{
    borrow::Cow,
    char, fs, io,
    path::{Path, PathBuf},
};

/// Largest hive file accepted: the format's offsets are 32 bits wide.
const MAX_FILE: u64 = 1 << 32;

/// The base block; cell offsets count from its end.
const BASE_BLOCK: usize = 0x1000;
const CHECKSUM_AT: usize = 0x1FC;

/// Value data above this size is split into `db` segments (hive format 1.4 and later).
const BIG_DATA: usize = 16344;
const KEY_COMP_NAME: u16 = 0x20;
const VALUE_COMP_NAME: u16 = 0x1;
const DATA_INLINE: u32 = 0x8000_0000;

/// `nk` fields, from the start of the cell's payload.
mod nk {
    pub(super) const FLAGS: usize = 0x02;
    pub(super) const LAST_WRITTEN: usize = 0x04;
    pub(super) const SUBKEY_COUNT: usize = 0x14;
    pub(super) const SUBKEY_LIST: usize = 0x1C;
    pub(super) const VALUE_COUNT: usize = 0x24;
    pub(super) const VALUE_LIST: usize = 0x28;
    pub(super) const NAME_LEN: usize = 0x48;
    pub(super) const NAME: usize = 0x4C;
}

/// `vk` fields, from the start of the cell's payload.
mod vk {
    pub(super) const NAME_LEN: usize = 0x02;
    pub(super) const DATA_LEN: usize = 0x04;
    pub(super) const DATA: usize = 0x08;
    pub(super) const KIND: usize = 0x0C;
    pub(super) const FLAGS: usize = 0x10;
    pub(super) const NAME: usize = 0x14;
}

#[derive(Debug, thiserror::Error)]
pub enum RegfError {
    #[error("can't read hive {}: {source}", path.display())]
    Io { path: PathBuf, source: io::Error },
    #[error("hive {} is too large ({size} bytes)", path.display())]
    TooLarge { path: PathBuf, size: u64 },
    #[error("not a registry hive: {0}")]
    NotAHive(&'static str),
    #[error("unsupported hive format {major}.{minor}")]
    Unsupported { major: u32, minor: u32 },
    #[error("corrupt hive: {what} at offset {offset:#x}")]
    Corrupt { what: &'static str, offset: u32 },
}

type Result<T> = std::result::Result<T, RegfError>;

fn corrupt<T>(what: &'static str, offset: u32) -> Result<T> {
    Err(RegfError::Corrupt { what, offset })
}

fn array<const N: usize>(bytes: &[u8], at: usize) -> Option<[u8; N]> {
    bytes.get(at..at.checked_add(N)?)?.try_into().ok()
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    array(bytes, at).map(u16::from_le_bytes)
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    array(bytes, at).map(u32::from_le_bytes)
}

/// XOR of the base block's first 127 dwords, with 0 and all-ones reserved.
fn checksum(base: &[u8]) -> Option<u32> {
    let sum = base
        .get(..CHECKSUM_AT)?
        .as_chunks::<4>()
        .0
        .iter()
        .fold(0, |sum, dword| sum ^ u32::from_le_bytes(*dword));
    Some(match sum {
        u32::MAX => u32::MAX - 1,
        0 => 1,
        sum => sum,
    })
}

/// A hive file in memory.
#[derive(Debug)]
pub struct Regf {
    data: Vec<u8>,
    root: u32,
    minor: u32,
    dirty: bool,
}

impl Regf {
    /// Reads a hive file. It is never written, and it stays usable by others while it's read.
    ///
    /// # Errors
    /// The file can't be read, or isn't a hive this reader supports.
    pub fn open(path: &Path) -> Result<Self> {
        let io_error = |source| RegfError::Io {
            path: path.to_owned(),
            source,
        };
        let size = fs::metadata(path).map_err(io_error)?.len();
        if size > MAX_FILE {
            return Err(RegfError::TooLarge {
                path: path.to_owned(),
                size,
            });
        }
        Self::parse(fs::read(path).map_err(io_error)?)
    }

    /// Checks the base block of a hive image. Cells are checked as they're reached.
    ///
    /// # Errors
    /// The image isn't a primary hive of a supported format, or it's truncated.
    pub fn parse(mut data: Vec<u8>) -> Result<Self> {
        let field = |at| u32_at(&data, at).ok_or(RegfError::NotAHive("truncated base block"));
        if !data.starts_with(b"regf") {
            return Err(RegfError::NotAHive("bad signature"));
        }
        if checksum(&data) != Some(field(CHECKSUM_AT)?) {
            return Err(RegfError::NotAHive("bad base block checksum"));
        }
        let (sequence, written) = (field(0x04)?, field(0x08)?);
        let (major, minor) = (field(0x14)?, field(0x18)?);
        if major != 1 || !(3..=6).contains(&minor) {
            return Err(RegfError::Unsupported { major, minor });
        }
        // 0: a primary file, 1: the direct-memory-load layout. Logs and alternates fail here.
        if field(0x1C)? != 0 || field(0x20)? != 1 {
            return Err(RegfError::NotAHive("not a primary hive file"));
        }
        let root = field(0x24)?;
        let bins = field(0x28)?;
        let end = usize::try_from(bins)
            .ok()
            .and_then(|bins| bins.checked_add(BASE_BLOCK))
            .filter(|end| *end <= data.len());
        let Some(end) = end else {
            return corrupt("hive shorter than its header says", bins);
        };
        data.truncate(end);
        Ok(Self {
            data,
            root,
            minor,
            dirty: sequence != written,
        })
    }

    /// The hive wasn't closed cleanly: its newest data is in the `.LOG1`/`.LOG2` files, which
    /// this reader doesn't apply.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// # Errors
    /// The root cell is corrupt.
    pub fn root(&self) -> Result<Key<'_>> {
        self.key_at(self.root)
    }

    /// The key at a `\`-separated path below the root; `Ok(None)` if it doesn't exist. Names
    /// compare case-insensitively, as the registry does.
    ///
    /// # Errors
    /// A cell on the way is corrupt.
    pub fn key(&self, path: &str) -> Result<Option<Key<'_>>> {
        let mut key = self.root()?;
        for part in path.split('\\').filter(|part| !part.is_empty()) {
            match key.subkey(part)? {
                Some(next) => key = next,
                None => return Ok(None),
            }
        }
        Ok(Some(key))
    }

    /// The payload of the allocated cell at `offset`.
    fn cell(&self, offset: u32) -> Result<&[u8]> {
        let Some(at) = usize::try_from(offset)
            .ok()
            .and_then(|o| o.checked_add(BASE_BLOCK))
        else {
            return corrupt("cell offset out of range", offset);
        };
        let Some(size) = array(&self.data, at).map(i32::from_le_bytes) else {
            return corrupt("cell offset out of range", offset);
        };
        // Allocated cells store their size negated, and it counts the size field itself.
        if size >= 0 {
            return corrupt("reference to a free cell", offset);
        }
        let payload = usize::try_from(size.unsigned_abs())
            .ok()
            .and_then(|size| Some(at.checked_add(4)?..at.checked_add(size)?))
            .and_then(|range| self.data.get(range));
        match payload {
            Some(payload) => Ok(payload),
            None => corrupt("cell runs past the hive", offset),
        }
    }

    fn key_at(&self, offset: u32) -> Result<Key<'_>> {
        let nk = self.cell(offset)?;
        if !nk.starts_with(b"nk") || nk.len() < nk::NAME {
            return corrupt("expected a key cell", offset);
        }
        Ok(Key {
            hive: self,
            nk,
            offset,
        })
    }

    fn value_at(&self, offset: u32) -> Result<Value<'_>> {
        let vk = self.cell(offset)?;
        if !vk.starts_with(b"vk") || vk.len() < vk::NAME {
            return corrupt("expected a value cell", offset);
        }
        Ok(Value {
            hive: self,
            vk,
            offset,
        })
    }

    /// Every entry of a subkey list costs at least one `nk` cell in a real hive, so a count
    /// beyond this can't be honest.
    fn max_subkeys(&self) -> usize {
        self.data.len() / (nk::NAME + 4)
    }
}

/// A key name as stored: Latin-1 when every character fits, else UTF-16LE.
#[derive(Debug, Clone, Copy)]
enum Name<'a> {
    Latin1(&'a [u8]),
    Utf16(&'a [u8]),
}

impl Name<'_> {
    fn eq_ignore_case(self, other: &str) -> bool {
        match self {
            Self::Latin1(bytes) => same_chars(bytes.iter().map(|b| char::from(*b)), other),
            Self::Utf16(bytes) => same_chars(utf16_chars(bytes), other),
        }
    }

    fn to_owned_string(self) -> String {
        match self {
            Self::Latin1(bytes) => bytes.iter().map(|b| char::from(*b)).collect(),
            Self::Utf16(bytes) => utf16_chars(bytes).collect(),
        }
    }
}

fn utf16_chars(bytes: &[u8]) -> impl Iterator<Item = char> {
    let units = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c));
    char::decode_utf16(units).map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
}

/// The registry's comparison: per-character uppercase, with no expansion (`ß` stays `ß`).
fn same_chars(chars: impl Iterator<Item = char>, other: &str) -> bool {
    fn upper(c: char) -> char {
        let mut upper = c.to_uppercase();
        match (upper.next(), upper.next()) {
            (Some(single), None) => single,
            _ => c,
        }
    }
    chars.map(upper).eq(other.chars().map(upper))
}

/// A key (`nk` cell).
#[derive(Debug, Clone, Copy)]
pub struct Key<'a> {
    hive: &'a Regf,
    nk: &'a [u8],
    offset: u32,
}

impl<'a> Key<'a> {
    fn field(&self, at: usize) -> Result<u32> {
        u32_at(self.nk, at).map_or_else(|| corrupt("short key cell", self.offset), Ok)
    }

    fn flags(&self) -> Result<u16> {
        u16_at(self.nk, nk::FLAGS).map_or_else(|| corrupt("short key cell", self.offset), Ok)
    }

    fn stored_name(&self) -> Result<Name<'a>> {
        let name = u16_at(self.nk, nk::NAME_LEN)
            .and_then(|len| self.nk.get(nk::NAME..nk::NAME + usize::from(len)));
        let Some(name) = name else {
            return corrupt("key name runs past its cell", self.offset);
        };
        Ok(if self.flags()? & KEY_COMP_NAME == 0 {
            Name::Utf16(name)
        } else {
            Name::Latin1(name)
        })
    }

    /// # Errors
    /// The name runs past its cell.
    pub fn name(&self) -> Result<String> {
        Ok(self.stored_name()?.to_owned_string())
    }

    /// Last write time, in `FILETIME` ticks.
    ///
    /// # Errors
    /// The cell is too short.
    pub fn last_written(&self) -> Result<u64> {
        array(self.nk, nk::LAST_WRITTEN).map_or_else(
            || corrupt("short key cell", self.offset),
            |bytes| Ok(u64::from_le_bytes(bytes)),
        )
    }

    /// The direct subkey called `name`; `Ok(None)` if there's none.
    ///
    /// # Errors
    /// A subkey list or key cell is corrupt.
    pub fn subkey(&self, name: &str) -> Result<Option<Self>> {
        let mut found = None;
        self.each_subkey(&mut |key| {
            if key.stored_name()?.eq_ignore_case(name) {
                found = Some(key);
                return Ok(false);
            }
            Ok(true)
        })?;
        Ok(found)
    }

    /// The direct subkeys, in hive order.
    ///
    /// # Errors
    /// A subkey list or key cell is corrupt.
    pub fn subkeys(&self) -> Result<Vec<Self>> {
        let mut keys = Vec::new();
        self.each_subkey(&mut |key| {
            keys.push(key);
            Ok(true)
        })?;
        Ok(keys)
    }

    /// Visits the subkeys until `visit` answers `false`.
    fn each_subkey(&self, visit: &mut dyn FnMut(Self) -> Result<bool>) -> Result<()> {
        let count = self.field(nk::SUBKEY_COUNT)?;
        if count == 0 {
            return Ok(());
        }
        let left = usize::try_from(count)
            .ok()
            .filter(|count| *count <= self.hive.max_subkeys());
        let Some(mut left) = left else {
            return corrupt("impossible subkey count", self.offset);
        };
        let list = self.field(nk::SUBKEY_LIST)?;
        self.walk_list(list, true, &mut left, visit)?;
        Ok(())
    }

    /// `left` caps the keys visited, so lists that repeat each other stay linear. Answers
    /// `false` once `visit` has stopped.
    fn walk_list(
        &self,
        offset: u32,
        top: bool,
        left: &mut usize,
        visit: &mut dyn FnMut(Self) -> Result<bool>,
    ) -> Result<bool> {
        let list = self.hive.cell(offset)?;
        // li: offsets. lf, lh: offset + name hint. ri: offsets of those lists, one level only.
        let (step, index) = match list.get(..2) {
            Some(b"li") => (4, false),
            Some(b"lf" | b"lh") => (8, false),
            Some(b"ri") if top => (4, true),
            _ => return corrupt("expected a subkey list", offset),
        };
        let entries = u16_at(list, 2)
            .and_then(|count| usize::from(count).checked_mul(step))
            .and_then(|bytes| list.get(4..bytes.checked_add(4)?));
        let Some(entries) = entries else {
            return corrupt("subkey list runs past its cell", offset);
        };
        for entry in entries.chunks_exact(step) {
            let Some(target) = u32_at(entry, 0) else {
                return corrupt("subkey list runs past its cell", offset);
            };
            let go_on = if index {
                self.walk_list(target, false, left, visit)?
            } else {
                let Some(remaining) = left.checked_sub(1) else {
                    return corrupt("more subkeys than the key declares", self.offset);
                };
                *left = remaining;
                visit(self.hive.key_at(target)?)?
            };
            if !go_on {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// The value called `name` (`""` for the default value); `Ok(None)` if there's none.
    ///
    /// # Errors
    /// The value list or a value cell is corrupt.
    pub fn value(&self, name: &str) -> Result<Option<Value<'a>>> {
        for offset in self.value_offsets()? {
            let value = self.hive.value_at(u32::from_le_bytes(*offset))?;
            if value.stored_name()?.eq_ignore_case(name) {
                return Ok(Some(value));
            }
        }
        Ok(None)
    }

    /// The key's values, in hive order.
    ///
    /// # Errors
    /// The value list or a value cell is corrupt.
    pub fn values(&self) -> Result<Vec<Value<'a>>> {
        self.value_offsets()?
            .iter()
            .map(|offset| self.hive.value_at(u32::from_le_bytes(*offset)))
            .collect()
    }

    fn value_offsets(&self) -> Result<&'a [[u8; 4]]> {
        let count = self.field(nk::VALUE_COUNT)?;
        if count == 0 {
            return Ok(&[]);
        }
        let list = self.hive.cell(self.field(nk::VALUE_LIST)?)?;
        let bytes = usize::try_from(count)
            .ok()
            .and_then(|count| count.checked_mul(4));
        match bytes.and_then(|bytes| list.get(..bytes)) {
            Some(offsets) => Ok(offsets.as_chunks::<4>().0),
            None => corrupt("value list runs past its cell", self.offset),
        }
    }
}

/// A value (`vk` cell). The name and data are read on demand.
#[derive(Debug, Clone, Copy)]
pub struct Value<'a> {
    hive: &'a Regf,
    vk: &'a [u8],
    offset: u32,
}

impl<'a> Value<'a> {
    fn field(&self, at: usize) -> Result<u32> {
        u32_at(self.vk, at).map_or_else(|| corrupt("short value cell", self.offset), Ok)
    }

    fn stored_name(&self) -> Result<Name<'a>> {
        let name = u16_at(self.vk, vk::NAME_LEN)
            .and_then(|len| self.vk.get(vk::NAME..vk::NAME + usize::from(len)));
        let flags = u16_at(self.vk, vk::FLAGS);
        let (Some(name), Some(flags)) = (name, flags) else {
            return corrupt("value name runs past its cell", self.offset);
        };
        Ok(if flags & VALUE_COMP_NAME == 0 {
            Name::Utf16(name)
        } else {
            Name::Latin1(name)
        })
    }

    /// # Errors
    /// The name runs past its cell.
    pub fn name(&self) -> Result<String> {
        Ok(self.stored_name()?.to_owned_string())
    }

    /// The `REG_*` type.
    ///
    /// # Errors
    /// The cell is too short.
    pub fn kind(&self) -> Result<u32> {
        self.field(vk::KIND)
    }

    /// The data: inline, in one cell, or joined from `db` segments.
    ///
    /// # Errors
    /// The data is corrupt.
    pub fn data(&self) -> Result<Cow<'a, [u8]>> {
        let raw = self.field(vk::DATA_LEN)?;
        let Ok(len) = usize::try_from(raw & !DATA_INLINE) else {
            return corrupt("impossible data size", self.offset);
        };
        let slice = |bytes: &'a [u8]| match bytes.get(..len) {
            Some(data) => Ok(Cow::Borrowed(data)),
            None => corrupt("value data runs past its cell", self.offset),
        };
        if raw & DATA_INLINE != 0 {
            return slice(self.vk.get(vk::DATA..vk::DATA + 4).unwrap_or_default());
        }
        if len == 0 {
            return Ok(Cow::Borrowed(&[]));
        }
        let cell = self.hive.cell(self.field(vk::DATA)?)?;
        if len > BIG_DATA && self.hive.minor >= 4 && cell.starts_with(b"db") {
            return self.joined(cell, len).map(Cow::Owned);
        }
        slice(cell)
    }

    /// Joins `db` segments. Each one holds `BIG_DATA` bytes but the last, and a segment may
    /// be listed twice in a crafted hive, so `len` is checked against the file first.
    fn joined(&self, db: &[u8], len: usize) -> Result<Vec<u8>> {
        if len > self.hive.data.len() {
            return corrupt("impossible data size", self.offset);
        }
        let list = u16_at(db, 2).zip(u32_at(db, 4));
        let Some((segments, list)) = list else {
            return corrupt("short big-data cell", self.offset);
        };
        let Some(offsets) = self.hive.cell(list)?.get(..usize::from(segments) * 4) else {
            return corrupt("big-data list runs past its cell", list);
        };
        let mut data = Vec::with_capacity(len);
        for offset in offsets.as_chunks::<4>().0 {
            let offset = u32::from_le_bytes(*offset);
            let segment = self.hive.cell(offset)?;
            let take = BIG_DATA.min(len - data.len());
            let Some(part) = segment.get(..take) else {
                return corrupt("short big-data segment", offset);
            };
            data.extend_from_slice(part);
        }
        if data.len() == len {
            Ok(data)
        } else {
            corrupt("big-data segments end early", list)
        }
    }
}

#[cfg(test)]
#[path = "regf/testkit.rs"]
pub(crate) mod testkit;

#[cfg(test)]
#[path = "regf/tests.rs"]
mod tests;
