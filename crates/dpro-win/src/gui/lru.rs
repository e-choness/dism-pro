//! A least-recently-used cache bounded by the total weight of its values (TD-0009 §5.2): glyph
//! coverage masks by their bytes, capped at 8 MiB (U13), and text layouts by their count.

use std::{collections::HashMap, hash::Hash};

pub(crate) struct Lru<K, V> {
    map: HashMap<K, Slot<V>>,
    /// The sum of the values' weights; never above `cap` after a call returns.
    weight: usize,
    cap: usize,
    /// Advances on every lookup; a slot remembers when it was last used.
    clock: u64,
}

struct Slot<V> {
    value: V,
    weight: usize,
    used: u64,
}

impl<K: Hash + Eq + Copy, V> Lru<K, V> {
    pub(crate) fn new(cap: usize) -> Self {
        Self {
            map: HashMap::new(),
            weight: 0,
            cap,
            clock: 0,
        }
    }

    /// The value for `key`. On a miss, `make` gives the value and its weight. `None` if `make`
    /// fails, or if the value alone outweighs the cap; nothing is cached then.
    ///
    /// Going over the cap evicts the least recently used values down to three quarters of it, so
    /// a full cache doesn't evict on every miss. The value just made is never evicted.
    pub(crate) fn get_or_insert_with(
        &mut self,
        key: K,
        make: impl FnOnce() -> Option<(V, usize)>,
    ) -> Option<&V> {
        self.clock += 1;
        if let Some(slot) = self.map.get_mut(&key) {
            slot.used = self.clock;
        } else {
            let (value, weight) = make()?;
            if weight > self.cap {
                return None;
            }
            self.map.insert(
                key,
                Slot {
                    value,
                    weight,
                    used: self.clock,
                },
            );
            self.weight += weight;
            if self.weight > self.cap {
                self.evict(key);
            }
        }
        self.map.get(&key).map(|slot| &slot.value)
    }

    /// Drops the least recently used values, except `keep`, until the weight is at most three
    /// quarters of the cap.
    fn evict(&mut self, keep: K) {
        let target = self.cap / 4 * 3;
        let mut by_age: Vec<(u64, K)> = self
            .map
            .iter()
            .filter(|&(k, _)| *k != keep)
            .map(|(k, slot)| (slot.used, *k))
            .collect();
        by_age.sort_unstable_by_key(|&(used, _)| used);
        for (_, k) in by_age {
            if self.weight <= target {
                break;
            }
            if let Some(slot) = self.map.remove(&k) {
                self.weight -= slot.weight;
            }
        }
    }

    pub(crate) fn weight(&self) -> usize {
        self.weight
    }

    pub(crate) fn len(&self) -> usize {
        self.map.len()
    }

    #[cfg(test)]
    pub(crate) fn contains(&self, key: &K) -> bool {
        self.map.contains_key(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A value of `w` bytes, as a glyph's coverage would be.
    fn bytes(w: usize) -> (Vec<u8>, usize) {
        (vec![0; w], w)
    }

    #[test]
    fn the_weight_never_exceeds_the_cap() {
        // U13's cap, filled many times over with glyph-sized values of varied sizes.
        let cap = 8 << 20;
        let mut lru = Lru::new(cap);
        for k in 0..100_000_usize {
            let w = 64 + k % 1_500;
            assert!(lru.get_or_insert_with(k, || Some(bytes(w))).is_some());
            assert!(lru.weight() <= cap, "{} > {cap} after {k}", lru.weight());
        }
        assert!(lru.weight() > cap / 2, "evicted far more than needed");
    }

    #[test]
    fn hits_keep_values_and_the_oldest_go_first() {
        let mut lru = Lru::new(400);
        for k in 0..4 {
            lru.get_or_insert_with(k, || Some(bytes(100)));
        }
        // Touch 0, so 1 is now the oldest.
        let mut made = false;
        lru.get_or_insert_with(0, || {
            made = true;
            Some(bytes(100))
        });
        assert!(!made, "a hit doesn't make the value again");
        lru.get_or_insert_with(4, || Some(bytes(100)));
        // 500 > 400: evict to 300, oldest first (1, then 2); 0 was used recently.
        assert_eq!(lru.weight(), 300);
        assert!(!lru.contains(&1) && !lru.contains(&2));
        assert!(lru.contains(&0) && lru.contains(&3) && lru.contains(&4));
    }

    #[test]
    fn failures_and_oversized_values_are_not_cached() {
        let mut lru: Lru<u32, Vec<u8>> = Lru::new(100);
        assert!(lru.get_or_insert_with(1, || None).is_none());
        assert!(lru.get_or_insert_with(2, || Some(bytes(101))).is_none());
        assert_eq!((lru.len(), lru.weight()), (0, 0));
        // A value of the whole cap fits, and evicts everything else.
        lru.get_or_insert_with(3, || Some(bytes(10)));
        assert!(lru.get_or_insert_with(4, || Some(bytes(100))).is_some());
        assert_eq!((lru.len(), lru.weight()), (1, 100));
    }
}
