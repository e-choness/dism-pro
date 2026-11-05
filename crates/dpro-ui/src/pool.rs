//! Spare text buffers (TD-0009 §7 U8).
//!
//! A freed node's text buffer goes to this pool, sorted by capacity, and a new node takes the
//! smallest one that already holds its text. Buffers grow in powers of two from [`MIN`] bytes, so a
//! class holds one capacity. Which slot a node gets doesn't matter, then: once a window has shown a
//! page, building it again finds a buffer for every text and never reallocates.

/// The smallest buffer: most labels fit.
const MIN: usize = 16;
/// Classes 16 B to 32 KiB. Freed buffers past that are dropped.
const CLASSES: usize = 12;

/// The capacity a buffer for `len` bytes is given.
pub(crate) fn capacity_for(len: usize) -> usize {
    len.max(MIN).next_power_of_two()
}

/// The class of a buffer of `capacity` (rounded down), or of the buffer `len` bytes need when
/// `capacity_for(len)` is passed.
fn class(capacity: usize) -> usize {
    usize::try_from(capacity.max(MIN).ilog2() - MIN.ilog2()).unwrap_or(CLASSES)
}

#[derive(Debug, Default)]
pub(crate) struct TextPool {
    classes: [Vec<String>; CLASSES],
}

impl TextPool {
    /// An empty buffer holding at least `len` bytes, if one is spare; otherwise an empty `String`,
    /// which allocates when the text is copied in.
    pub(crate) fn take(&mut self, len: usize) -> String {
        if len == 0 {
            return String::new();
        }
        let first = class(capacity_for(len));
        self.classes
            .iter_mut()
            .skip(first)
            .find_map(Vec::pop)
            .unwrap_or_default()
    }

    /// Keeps `s` for reuse.
    pub(crate) fn put(&mut self, mut s: String) {
        if s.capacity() == 0 {
            return;
        }
        let k = class(s.capacity());
        if let Some(spare) = self.classes.get_mut(k) {
            s.clear();
            spare.push(s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacities_are_powers_of_two_from_16() {
        assert_eq!(capacity_for(0), 16);
        assert_eq!(capacity_for(16), 16);
        assert_eq!(capacity_for(17), 32);
        assert_eq!(capacity_for(1000), 1024);
        assert_eq!((class(16), class(31), class(32), class(1024)), (0, 0, 1, 6));
    }

    #[test]
    fn takes_the_smallest_buffer_that_fits() {
        let mut pool = TextPool::default();
        for cap in [16, 64, 32] {
            pool.put(String::with_capacity(cap));
        }
        assert_eq!(pool.take(20).capacity(), 32);
        assert_eq!(pool.take(20).capacity(), 64);
        // Nothing big enough is left; the 16-byte buffer stays for a short text.
        assert_eq!(pool.take(20).capacity(), 0);
        assert_eq!(pool.take(3).capacity(), 16);
        assert_eq!(pool.take(0).capacity(), 0);
    }

    #[test]
    fn buffers_are_cleared_and_huge_or_empty_ones_dropped() {
        let mut pool = TextPool::default();
        pool.put(String::new());
        pool.put(String::with_capacity(1 << 20));
        let mut s = String::with_capacity(16);
        s.push_str("old");
        pool.put(s);
        let s = pool.take(1);
        assert_eq!((s.as_str(), s.capacity()), ("", 16));
        assert_eq!(pool.take(1).capacity(), 0);
    }
}
