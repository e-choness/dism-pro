//! Units as types (coding standards §2): never `size_mb: f64`.

use std::iter::Sum;

use serde::{Deserialize, Serialize};

/// A byte count. Arithmetic saturates: sizes are estimates and must never wrap.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Bytes(pub u64);

impl Bytes {
    pub const ZERO: Self = Self(0);

    #[must_use]
    pub const fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }
}

impl Sum for Bytes {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::ZERO, Self::saturating_add)
    }
}

/// A point in time as milliseconds since the Unix epoch (UTC). Used instead of
/// `SystemTime` so plans and journals serialize identically on every platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnixMillis(pub u64);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_saturate() {
        let total: Bytes = [Bytes(u64::MAX), Bytes(1)].into_iter().sum();
        assert_eq!(total, Bytes(u64::MAX));
        assert_eq!(Bytes(2).saturating_add(Bytes(3)), Bytes(5));
    }
}
