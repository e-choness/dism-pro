//! Cooperative cancellation (TD-0001 §5.3). `dpro-win` bridges a token to a Win32 event
//! for APIs that take a cancel `HANDLE`; WIMGAPI is cancelled from its message callback.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crate::Error;

/// Shared cancel flag. Cloning shares the flag; cancelling is permanent.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests cancellation. Jobs stop at the next change boundary.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    /// `Err(Error::Cancelled)` once cancelled; call between changes.
    ///
    /// # Errors
    /// [`Error::Cancelled`] if [`CancelToken::cancel`] was called on any clone.
    pub fn check(&self) -> Result<(), Error> {
        if self.is_cancelled() { Err(Error::Cancelled) } else { Ok(()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clones_share_the_flag() {
        let a = CancelToken::new();
        let b = a.clone();
        assert!(a.check().is_ok());
        b.cancel();
        assert!(a.is_cancelled());
        assert!(matches!(a.check(), Err(Error::Cancelled)));
    }
}
