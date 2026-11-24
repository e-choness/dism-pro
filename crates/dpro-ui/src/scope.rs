//! Scopes (TD-0009 A3.5 addition 10): a container that takes the Esc nothing inside used, and
//! gives focus back to where it was when it goes.
//!
//! Slice 6d's MEASURE stub: a scope is a plain container yet, and Esc passes it by.

use crate::{Element, Key};

impl<M> Element<'_, M> {
    /// A scope: a container (the drawer, the in-app picker) that takes the Esc its focused
    /// control doesn't use, and gives focus back when it goes.
    #[must_use]
    pub fn scope(key: Key) -> Self {
        Self::stack(key)
    }

    /// What Esc sends while focus is in this scope, or while it's the window's open scope.
    /// Other elements ignore it.
    #[must_use]
    pub fn on_escape(self, on_escape: fn() -> M) -> Self {
        let _ = on_escape;
        self
    }
}
