//! Per-plan string arena (TD-0001 §7.3). Paths, registry names and other strings are
//! stored once; changes refer to them by [`Atom`].
//!
//! Strings are bytes: Windows paths arrive as WTF-8 from `dpro-win` and may not be valid
//! UTF-8. Deduplication keys on a 64-bit hash with a collision chain, so the index costs
//! ~12 bytes per distinct string instead of a second copy of every string.

use std::{
    borrow::Cow,
    collections::HashMap,
    hash::{DefaultHasher, Hash, Hasher},
};

use crate::Error;

/// Index of a string in an [`Interner`]. Only meaningful with the interner that made it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Atom(u32);

impl Atom {
    /// The raw index, for error messages.
    pub(crate) const fn raw(self) -> u32 {
        self.0
    }
}

/// An [`Atom`] that names a filesystem path.
pub type PathId = Atom;

const NO_NEXT: u32 = u32::MAX;

#[derive(Debug, Clone, Default)]
pub struct Interner {
    bytes: Vec<u8>,
    /// `ends[i]` = end offset of atom `i` in `bytes`; atom `i` starts at `ends[i - 1]`.
    ends: Vec<u32>,
    /// Hash → first atom with that hash.
    heads: HashMap<u64, u32>,
    /// Next atom with the same hash, or `NO_NEXT`.
    next: Vec<u32>,
}

impl Interner {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of distinct strings.
    #[must_use]
    pub fn len(&self) -> usize {
        self.ends.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ends.is_empty()
    }

    /// Bytes of string data held (excluding the index).
    #[must_use]
    pub fn data_len(&self) -> usize {
        self.bytes.len()
    }

    /// Returns the atom for `s`, adding it if new.
    ///
    /// # Errors
    /// [`Error::ArenaFull`] past 4 GiB of string data or 2³² − 1 strings.
    pub fn intern(&mut self, s: &[u8]) -> Result<Atom, Error> {
        let hash = hash(s);
        let mut cursor = self.heads.get(&hash).copied().unwrap_or(NO_NEXT);
        while cursor != NO_NEXT {
            let atom = Atom(cursor);
            if self.get(atom) == Some(s) {
                return Ok(atom);
            }
            cursor = self.next[index(atom)];
        }

        let id = u32::try_from(self.ends.len()).map_err(|_| Error::ArenaFull)?;
        if id == NO_NEXT {
            return Err(Error::ArenaFull);
        }
        let end = self.bytes.len().checked_add(s.len()).ok_or(Error::ArenaFull)?;
        let end = u32::try_from(end).map_err(|_| Error::ArenaFull)?;
        self.bytes.extend_from_slice(s);
        self.ends.push(end);
        let previous_head = self.heads.insert(hash, id).unwrap_or(NO_NEXT);
        self.next.push(previous_head);
        Ok(Atom(id))
    }

    /// Convenience for UTF-8 input.
    ///
    /// # Errors
    /// See [`Interner::intern`].
    pub fn intern_str(&mut self, s: &str) -> Result<Atom, Error> {
        self.intern(s.as_bytes())
    }

    /// The bytes of `atom`, or `None` if it doesn't belong to this interner.
    #[must_use]
    pub fn get(&self, atom: Atom) -> Option<&[u8]> {
        let i = index(atom);
        let end = usize::try_from(*self.ends.get(i)?).ok()?;
        let start = match i.checked_sub(1) {
            Some(prev) => usize::try_from(self.ends[prev]).ok()?,
            None => 0,
        };
        self.bytes.get(start..end)
    }

    /// The string for `atom`, with invalid UTF-8 (unpaired surrogates in WTF-8) replaced.
    ///
    /// # Errors
    /// [`Error::UnknownAtom`] if `atom` doesn't belong to this interner.
    pub fn resolve_lossy(&self, atom: Atom) -> Result<Cow<'_, str>, Error> {
        self.get(atom)
            .map(String::from_utf8_lossy)
            .ok_or(Error::UnknownAtom(atom.raw()))
    }
}

fn index(atom: Atom) -> usize {
    // u32 → usize never truncates on the 64-bit targets we build for.
    usize::try_from(atom.0).unwrap_or(usize::MAX)
}

fn hash(s: &[u8]) -> u64 {
    // DefaultHasher::new() uses fixed keys: deterministic, which keeps benches stable.
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn deduplicates_and_resolves() {
        let mut i = Interner::new();
        let a = i.intern_str(r"C:\Windows\Temp\a.log").unwrap();
        let b = i.intern_str(r"C:\Windows\Temp\b.log").unwrap();
        assert_ne!(a, b);
        assert_eq!(i.intern_str(r"C:\Windows\Temp\a.log").unwrap(), a);
        assert_eq!(i.len(), 2);
        assert_eq!(i.resolve_lossy(b).unwrap(), r"C:\Windows\Temp\b.log");
        assert!(matches!(i.resolve_lossy(Atom(7)), Err(Error::UnknownAtom(7))));
    }

    #[test]
    fn empty_string_is_a_normal_atom() {
        let mut i = Interner::new();
        let e = i.intern(b"").unwrap();
        let x = i.intern(b"x").unwrap();
        assert_eq!(i.get(e), Some(&b""[..]));
        assert_eq!(i.get(x), Some(&b"x"[..]));
    }

    proptest! {
        #[test]
        fn every_string_round_trips(strings in proptest::collection::vec(any::<Vec<u8>>(), 0..200)) {
            let mut i = Interner::new();
            let atoms: Vec<Atom> = strings.iter().map(|s| i.intern(s).unwrap()).collect();
            for (s, a) in strings.iter().zip(&atoms) {
                prop_assert_eq!(i.get(*a), Some(s.as_slice()));
                prop_assert_eq!(i.intern(s).unwrap(), *a);
            }
            let distinct: std::collections::HashSet<&Vec<u8>> = strings.iter().collect();
            prop_assert_eq!(i.len(), distinct.len());
        }
    }
}
