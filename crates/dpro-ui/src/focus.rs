//! Focus restoration (TD-0009 A4.5, slice 7c): focus never falls back to the window.
//!
//! - A [`FocusMemo`] remembers the focused node by its keys from the root, so it finds the node
//!   again after a rebuild made it anew.
//! - A return stack of four memos: what opened a dialog gets focus back when it closes.
//!
//! The MEASURE commit's stubs: the benches and tests are written against this API first.

use crate::{Key, NodeId, Ui};

/// How many keys a [`FocusMemo`] holds: a node deeper than this has none.
const DEPTH: usize = 16;

/// The focused node, remembered by its keys from the root (TD-0009 A4.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FocusMemo {
    keys: [Key; DEPTH],
    len: u8,
}

impl FocusMemo {
    /// The keys from the root, the root's child first.
    #[must_use]
    pub fn keys(&self) -> &[Key] {
        &self.keys[..usize::from(self.len)]
    }
}

#[allow(
    clippy::unused_self,
    reason = "slice 7c's MEASURE stubs: the code lands after the benches"
)]
impl<M> Ui<M> {
    /// The focused node's memo; `None` with nothing focused, or past depth 16.
    #[must_use]
    pub fn focus_memo(&self) -> Option<FocusMemo> {
        None
    }

    /// Focuses the node `memo` names, or the nearest surviving ancestor's first focusable node.
    /// Returns the node focused.
    pub fn restore_focus(&mut self, _memo: &FocusMemo) -> Option<NodeId> {
        None
    }

    /// Remembers the focused node on the return stack, which holds four: the oldest drops.
    pub fn push_focus_return(&mut self) {}

    /// Gives focus back to the node last pushed ([`restore_focus`](Self::restore_focus)), and
    /// drops it from the stack. Returns the node focused; `None` with the stack empty.
    pub fn pop_focus_return(&mut self) -> Option<NodeId> {
        None
    }

    /// Focuses the first focusable node in `container`. Returns it.
    pub fn focus_first_in(&mut self, _container: NodeId) -> Option<NodeId> {
        None
    }

    /// Focuses the first focusable node in the Main landmark: where a page change from anywhere
    /// but the rail puts focus (A4.4). Returns it.
    pub fn focus_page_start(&mut self) -> Option<NodeId> {
        None
    }
}
