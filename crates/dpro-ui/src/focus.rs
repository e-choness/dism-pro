//! Focus restoration (TD-0009 A4.4, A4.5; slice 7c): focus never falls back to the window.
//!
//! - **The successor rule.** A removal that takes the focused node records where it was: its
//!   parent, and the sibling before it. When the build ends with nothing focused, focus goes to
//!   the first stop after that place among the siblings' subtrees, else the last before it, else
//!   the same search one level up. The search stops at the enclosing landmark, whose first stop
//!   (itself, as a viewport with none inside) is the last resort. Past a landmark with nothing to
//!   focus it goes on, so focus doesn't fall to the window (decision #41). The ring stays as it
//!   was. A table's own refocus (its current row, a header's neighbor) and a scope's give-back
//!   come first.
//! - **The parked focus** (`build.rs`, `input.rs`): a control disabled while focused keeps the
//!   focus, takes no activation, and Tab and Shift+Tab leave it.
//! - **Memos.** A [`FocusMemo`] remembers the focused node by its keys from the root, so it finds
//!   the node again after a rebuild made it anew. If the node went, focus goes to the nearest
//!   surviving ancestor's first stop, then the Main landmark's, then the window's.
//! - **The return stack** holds four memos; a fifth push drops the oldest. Dialogs push as they
//!   open and pop as they close (`platform`); A5's `Effect::Focus(Back)` pops.
//!
//! Scopes (`scope.rs`, the drawer) keep a memo of their own, taken as they open, rather than
//! pushing: a scope removed by a page change would leave the stack holding its entry, and its
//! give-back runs in the build that removes it, before the successor rule, so focus moves once.
//!
//! None of it runs on a steady frame: a removal pays a few comparisons, and a build one.

use crate::{
    Key, NodeId, Ui,
    tree::{LandmarkKind, NIL},
    ui::ROOT,
};

/// How many keys a [`FocusMemo`] holds: a node deeper than this has none.
const DEPTH: usize = 16;
/// How many memos the return stack holds (A4.7).
const RETURNS: usize = 4;

/// The focused node, remembered by its keys from the root (TD-0009 A4.5): it finds the node again
/// after a rebuild made it anew, as a sheet's page or a refreshed list does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FocusMemo {
    keys: [Key; DEPTH],
    len: u8,
}

impl FocusMemo {
    /// No node: restoring it focuses the Main landmark's first stop.
    pub(crate) const EMPTY: Self = Self {
        keys: [Key(0); DEPTH],
        len: 0,
    };

    /// A memo of the node at `path` from the root, as [`Ui::find`] takes it (A5's
    /// `FocusTarget::Key`); `None` past depth 16.
    #[must_use]
    pub fn from_keys(path: &[Key]) -> Option<Self> {
        let len = u8::try_from(path.len())
            .ok()
            .filter(|&n| usize::from(n) <= DEPTH)?;
        let mut memo = Self::EMPTY;
        memo.keys[..path.len()].copy_from_slice(path);
        memo.len = len;
        Some(memo)
    }

    /// The keys from the root, the root's child first.
    #[must_use]
    pub fn keys(&self) -> &[Key] {
        &self.keys[..usize::from(self.len)]
    }
}

/// Where the focused node, or the child of `parent` holding it, was: after `prev` ([`NIL`] for
/// the first child), both live until the build ends. `parent` is [`NIL`] when nothing is
/// recorded.
///
/// A reconcile that moves a later sibling ahead of an unmatched one leaves the unmatched one
/// behind every sibling placed, so where it is when it goes isn't where it was. The record is
/// taken when the first move passes it, and the removal keeps it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Lost {
    pub(crate) parent: u32,
    prev: u32,
    /// The child of `parent` the record is for.
    node: u32,
}

impl Lost {
    pub(crate) const NONE: Self = Self {
        parent: NIL,
        prev: NIL,
        node: NIL,
    };
}

/// What a `Ui` keeps off itself until first needed (most never close a scope or a dialog): the
/// return stack, and what a scope that went gives back.
#[derive(Debug)]
pub(crate) struct Restore {
    returns: [FocusMemo; RETURNS],
    len: u8,
    /// While a removal frees scopes: the outermost one's memo, until the removal reads it.
    pub(crate) freed: Option<FocusMemo>,
    /// A removal took focus with a scope: its memo, for the end of the build.
    pub(crate) pending: Option<FocusMemo>,
}

impl<M> Ui<M> {
    /// The focused node's memo; `None` with nothing focused, or past depth 16.
    #[must_use]
    pub fn focus_memo(&self) -> Option<FocusMemo> {
        if self.focus == NIL {
            return None;
        }
        let mut memo = FocusMemo::EMPTY;
        let mut len = 0;
        let mut at = self.focus;
        while at != ROOT {
            if at == NIL {
                return None;
            }
            *memo.keys.get_mut(len)? = self.nodes[at as usize].key;
            len += 1;
            at = self.nodes[at as usize].parent;
        }
        memo.keys[..len].reverse();
        memo.len = u8::try_from(len).ok()?;
        Some(memo)
    }

    /// Focuses the node `memo` names, found by its keys: a disabled one too, where focus parks
    /// (A4.4). If it went, or can't take focus, the nearest surviving ancestor's first focusable
    /// node, then the Main landmark's, then the window's. Returns the node focused; `None` if
    /// nothing can take focus, which leaves it as it was. The ring stays as it was.
    pub fn restore_focus(&mut self, memo: &FocusMemo) -> Option<NodeId> {
        let to = self.memo_target(memo);
        self.focus_to(to)
    }

    /// Remembers the focused node on the return stack (nothing focused is remembered too, and
    /// gives back the Main landmark's first control). It holds four: a fifth push drops the
    /// oldest. A dialog pushes as it opens (A4.5).
    pub fn push_focus_return(&mut self) {
        let memo = self.focus_memo().unwrap_or(FocusMemo::EMPTY);
        let r = self.restore_mut();
        if usize::from(r.len) == RETURNS {
            r.returns.copy_within(1.., 0);
            r.len -= 1;
        }
        r.returns[usize::from(r.len)] = memo;
        r.len += 1;
    }

    /// Gives focus back to the node last pushed, found by its keys, and drops it from the stack:
    /// a dialog that closes, A5's `Effect::Focus(Back)`. If that node went, a focus the successor
    /// rule already moved stays where it went (A4.4); with nothing focused, focus goes where
    /// [`restore_focus`](Self::restore_focus) falls back to. Returns the node focused; `None` with
    /// the stack empty, which leaves focus as it was.
    pub fn pop_focus_return(&mut self) -> Option<NodeId> {
        let r = self.restore.as_mut()?;
        r.len = r.len.checked_sub(1)?;
        let memo = r.returns[usize::from(r.len)];
        self.return_focus(&memo)
    }

    /// Focus given back by `memo`, as [`pop_focus_return`](Self::pop_focus_return) gives it: to
    /// its node if it's there; if not, a focus already on a node stays. The toast's way back too.
    pub(crate) fn return_focus(&mut self, memo: &FocusMemo) -> Option<NodeId> {
        let (at, whole) = self.memo_walk(memo);
        if self.focus != NIL && !(whole && at != ROOT && self.holds_focus(at)) {
            return self.id(self.focus);
        }
        self.restore_focus(memo)
    }

    /// Focuses the first focusable node in `container`, in Tab's order: `container` itself if
    /// it's a control, or a viewport with nothing focusable inside. Returns it; `None` if there's
    /// none, which leaves focus as it was.
    pub fn focus_first_in(&mut self, container: NodeId) -> Option<NodeId> {
        let c = self.index(container)?;
        let to = self.first_stop_in(c);
        self.focus_to(to)
    }

    /// Focuses the first focusable node in the Main landmark: where a page change from anywhere
    /// but the rail puts focus (A4.4, A4.10 item 4; A5's `Effect::Focus(PageStart)`). Returns it;
    /// `None` without a Main landmark or anything focusable in it, which leaves focus as it was.
    pub fn focus_page_start(&mut self) -> Option<NodeId> {
        let main = self.main_landmark();
        let to = if main == NIL {
            NIL
        } else {
            self.first_stop_in(main)
        };
        self.focus_to(to)
    }

    /// Focus moved to `to` (where it is already, it stays put); its id, or `None` for [`NIL`].
    fn focus_to(&mut self, to: u32) -> Option<NodeId> {
        if to == NIL {
            return None;
        }
        if to != self.focus {
            self.move_focus(to);
        }
        self.id(to)
    }

    /// The return stack and the scopes' memos, made on first use.
    pub(crate) fn restore_mut(&mut self) -> &mut Restore {
        self.restore.get_or_insert_with(|| {
            Box::new(Restore {
                returns: [FocusMemo::EMPTY; RETURNS],
                len: 0,
                freed: None,
                pending: None,
            })
        })
    }

    /// Where `memo` puts focus: its node, if it can hold focus; else the nearest surviving
    /// ancestor's first stop (the root's excepted), the Main landmark's, the window's.
    pub(crate) fn memo_target(&self, memo: &FocusMemo) -> u32 {
        let (mut at, whole) = self.memo_walk(memo);
        if whole && at != ROOT && self.holds_focus(at) {
            return at;
        }
        while at != ROOT {
            let stop = self.first_stop_in(at);
            if stop != NIL {
                return stop;
            }
            at = self.nodes[at as usize].parent;
        }
        let main = self.main_landmark();
        let stop = if main == NIL {
            NIL
        } else {
            self.first_stop_in(main)
        };
        if stop == NIL {
            self.tab_from(NIL, true)
        } else {
            stop
        }
    }

    /// The deepest node of `memo`'s path still there ([`ROOT`] for none), and whether it's the
    /// whole path.
    fn memo_walk(&self, memo: &FocusMemo) -> (u32, bool) {
        let mut at = ROOT;
        for &key in memo.keys() {
            match self
                .children_of(at)
                .find(|&c| self.nodes[c as usize].key == key)
            {
                Some(c) => at = c,
                None => return (at, false),
            }
        }
        (at, true)
    }

    /// Whether node `i` can hold focus given back to it: a control, enabled or not (focus parks
    /// on a disabled one), or a viewport with no stop inside; never a suggestion (A3.5 addition
    /// 7).
    fn holds_focus(&self, i: u32) -> bool {
        (self.nodes[i as usize].role.interactive() || self.lone_viewport(i)) && !self.suggestion(i)
    }

    /// The Main landmark, or [`NIL`]. There's one per page (A4.4); a sheet takes it away.
    fn main_landmark(&self) -> u32 {
        (0u32..)
            .zip(&self.nodes)
            .find(|&(i, n)| {
                n.live && n.has_props() && self.landmark_of(i) == Some(LandmarkKind::Main)
            })
            .map_or(NIL, |(i, _)| i)
    }

    /// The first Tab stop in `at`'s subtree, `at` included; [`NIL`] if none.
    fn first_stop_in(&self, at: u32) -> u32 {
        let stop = self.tab_stop(at, NIL, true);
        if stop == NIL {
            self.first_stop_below(at)
        } else {
            stop
        }
    }

    /// The last Tab stop in `at`'s subtree, in Tab's order, `at` included; [`NIL`] if none.
    fn last_stop_in(&self, at: u32) -> u32 {
        let deepest_last = |mut i: u32| {
            while self.nodes[i as usize].last != NIL {
                i = self.nodes[i as usize].last;
            }
            i
        };
        // Tree order backwards: a node's subtree before the node.
        let mut i = deepest_last(at);
        loop {
            let stop = self.tab_stop(i, NIL, false);
            if stop != NIL || i == at {
                return stop;
            }
            let n = &self.nodes[i as usize];
            i = if n.prev == NIL {
                n.parent
            } else {
                deepest_last(n.prev)
            };
        }
    }

    /// A removal took the focused node, or found a record of where an earlier one did, or freed
    /// a scope. `child` went from under `parent`, after `prev`; `focus` was focused before.
    #[cold]
    #[inline(never)]
    pub(crate) fn focus_removed(&mut self, child: u32, parent: u32, prev: u32, focus: u32) {
        if self.scope_freed {
            self.scope_removed(focus);
        }
        let here = Lost {
            parent,
            prev,
            node: child,
        };
        if focus != NIL && self.focus == NIL {
            // Where a move first passed it, if one did; else where it is.
            if self.lost.parent != parent || self.lost.node != child {
                self.lost = here;
            }
            self.focus_pending = true;
        } else if self.lost.parent != NIL {
            // A later removal in the same build: the record stays on live nodes.
            if !self.nodes[self.lost.parent as usize].live {
                // The place went with an ancestor of it: the place is where that ancestor was.
                self.lost = here;
            } else if self.lost.prev == child {
                self.lost.prev = prev;
            }
        }
    }

    /// A reconcile under `parent` moves a later sibling ahead of `cursor`, the first child not
    /// matched yet. If `cursor` is the focused node or holds it, its place is recorded, before
    /// the move leaves it behind (see [`Lost`]); the first move to pass it decides.
    #[cold]
    #[inline(never)]
    pub(crate) fn focus_passed(&mut self, parent: u32, cursor: u32) {
        if self.lost.parent == parent && self.lost.node == cursor {
            return;
        }
        let mut at = self.focus;
        while at != NIL && at != ROOT {
            let up = self.nodes[at as usize].parent;
            if up == parent {
                break;
            }
            at = up;
        }
        if at == cursor {
            self.lost = Lost {
                parent,
                prev: self.nodes[cursor as usize].prev,
                node: cursor,
            };
            // The build's end drops the record if focus stayed.
            self.focus_pending = true;
        }
    }

    /// The end of a build: focus a scope took with it goes back to the node it remembers, then a
    /// focus a removal took goes to its successor.
    #[cold]
    #[inline(never)]
    pub(crate) fn settle_focus(&mut self) {
        self.focus_pending = false;
        self.give_focus_back();
        let lost = std::mem::replace(&mut self.lost, Lost::NONE);
        if self.focus == NIL && lost.parent != NIL {
            let to = self.successor(lost);
            if to != NIL {
                self.move_focus(to);
            }
        }
    }

    /// The successor of a focused node that went from `lost` (A4.4): the first stop after its
    /// place among its siblings' subtrees, else the last before it, else the same one level up.
    /// At a landmark, its own stop (a viewport with none inside) is the last resort; with none,
    /// the search goes on past it. [`NIL`] if the window has no stop.
    fn successor(&self, lost: Lost) -> u32 {
        let mut parent = lost.parent;
        let (mut after, mut before) = if lost.prev == NIL {
            (self.nodes[parent as usize].first, NIL)
        } else {
            (self.nodes[lost.prev as usize].next, lost.prev)
        };
        loop {
            while after != NIL {
                let stop = self.stop_from(after, true);
                if stop != NIL {
                    return stop;
                }
                after = self.nodes[after as usize].next;
            }
            while before != NIL {
                let stop = self.stop_from(before, false);
                if stop != NIL {
                    return stop;
                }
                before = self.nodes[before as usize].prev;
            }
            if parent == ROOT {
                return NIL;
            }
            if self.landmark_of(parent).is_some() {
                let stop = self.tab_stop(parent, NIL, true);
                if stop != NIL {
                    return stop;
                }
            }
            let p = &self.nodes[parent as usize];
            (after, before, parent) = (p.next, p.prev, p.parent);
        }
    }

    /// Where the successor search lands in sibling `at`'s subtree: `at` itself if it's a control
    /// (a removed tab's neighbor, not its group's pick), else its first stop (`forward`) or last.
    fn stop_from(&self, at: u32, forward: bool) -> u32 {
        if self.nodes[at as usize].interactive() && !self.suggestion(at) {
            at
        } else if forward {
            self.first_stop_in(at)
        } else {
            self.last_stop_in(at)
        }
    }
}
