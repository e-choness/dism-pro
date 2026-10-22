//! Progress reporting (doc 02 §3). Native callbacks report through a sink; the engine
//! coalesces events to at most one per frame before they reach the UI.

/// Units of work done out of a total. `total == 0` means "unknown".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Progress {
    pub done: u64,
    pub total: u64,
}

impl Progress {
    /// Completion in permille (0..=1000), or `None` when the total is unknown.
    #[must_use]
    pub fn permille(self) -> Option<u16> {
        if self.total == 0 {
            return None;
        }
        let done = u128::from(self.done.min(self.total));
        let permille = done * 1000 / u128::from(self.total);
        u16::try_from(permille).ok()
    }
}

/// Receives progress. Implementations must be cheap: the budget is ≤ 1 µs per call
/// on the actor thread (TD-0001 §7.2).
pub trait ProgressSink {
    fn report(&self, progress: Progress);
}

/// Discards progress.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoProgress;

impl ProgressSink for NoProgress {
    fn report(&self, _progress: Progress) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permille_clamps_and_handles_unknown_totals() {
        assert_eq!(Progress { done: 1, total: 3 }.permille(), Some(333));
        assert_eq!(Progress { done: 9, total: 3 }.permille(), Some(1000));
        assert_eq!(Progress { done: 1, total: 0 }.permille(), None);
    }
}
