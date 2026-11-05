//! Application layer: view-models, commands, navigation, palette. View-models are plain data
//! with no toolkit types.
//!
//! Layer: application.
//! Allowed workspace dependencies: `dpro-core`, `dpro-rules`, `dpro-engine`.

#![forbid(unsafe_code)]

mod fold;
pub mod palette;
pub mod table;

/// A revision counter. Views compare it to skip rebuilding what didn't change (TD-0009 §5.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rev(u32);

impl Rev {
    /// Marks a change. Wraps: views only compare for equality.
    pub fn bump(&mut self) {
        self.0 = self.0.wrapping_add(1);
    }
}
