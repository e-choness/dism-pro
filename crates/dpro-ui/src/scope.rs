//! Scopes (TD-0009 A3.5 addition 10): a container that takes the Esc nothing inside used, and
//! gives focus back to where it was when it goes.
//!
//! - An Esc the focused control doesn't use goes to the nearest enclosing scope that has an
//!   escape; with focus outside every scope, to the window's open scope (the last opened).
//! - When a scope appears, the `Ui` records the focused node. When it's removed and focus went
//!   with it, focus goes back there if that node is still there at the end of the build, else to
//!   the first place Tab lands in the scope's parent. Focus the user moved out of the scope stays
//!   where it went.
//!
//! The drawer (A3.5 (6)) and the in-app picker use it. General focus restoration is A4's.

use crate::{
    Element, Key, NodeId, Ui,
    tree::{Extra, NIL},
};

/// An open scope: its node, what Esc sends it, the node focused when it appeared, and whether
/// it appeared in this build (its owner moves focus into it once).
#[derive(Debug)]
pub(crate) struct ScopeSlot<M> {
    node: u32,
    on_escape: Option<fn() -> M>,
    back: Option<NodeId>,
    opened: bool,
}

/// What a scope that went gives focus back to.
#[derive(Debug, Default)]
pub(crate) struct Closed {
    /// A removal is freeing a scope: set while it frees nodes, until the removal reads it.
    pub(crate) freed: bool,
    /// The outermost scope freed's recorded node.
    freed_back: Option<NodeId>,
    /// Set by a removal that took focus with a scope, for the end of the build.
    pending: Option<GiveBack>,
}

/// Where focus goes back to at the end of the build: the recorded node, else the first place
/// Tab lands in the scope's parent.
#[derive(Clone, Copy, Debug)]
struct GiveBack {
    back: Option<NodeId>,
    parent: Option<NodeId>,
}

impl<M> Element<'_, M> {
    /// A scope: a container (the drawer, the in-app picker) that takes the Esc its focused
    /// control doesn't use, and gives focus back when it goes.
    #[must_use]
    pub fn scope(key: Key) -> Self {
        let mut el = Self::stack(key);
        el.extra = Extra::Scope { on_escape: None };
        el
    }

    /// What Esc sends while focus is in this scope, or while it's the window's open scope.
    /// Other elements ignore it.
    #[must_use]
    pub fn on_escape(mut self, on_escape: fn() -> M) -> Self {
        if let Extra::Scope { .. } = self.extra {
            self.extra = Extra::Scope {
                on_escape: Some(on_escape),
            };
        }
        self
    }
}

impl<M> Ui<M> {
    /// Node `i`'s scope from its element: one that appeared records the focused node; one that
    /// stopped being a scope is forgotten, and gives nothing back.
    pub(crate) fn update_scope(&mut self, i: u32, scope: bool, on_escape: Option<fn() -> M>) {
        let slot = self.scopes.iter().position(|s| s.node == i);
        match (slot, scope) {
            (Some(k), true) => self.scopes[k].on_escape = on_escape,
            (None, true) => {
                let back = self.id(self.focus);
                self.scopes.push(ScopeSlot {
                    node: i,
                    on_escape,
                    back,
                    opened: true,
                });
            }
            (Some(k), false) => {
                self.scopes.remove(k);
            }
            (None, false) => {}
        }
    }

    /// Whether scope node `i` appeared since the last call: true once, for its owner to move
    /// focus into it.
    pub(crate) fn scope_opened(&mut self, i: u32) -> bool {
        self.scopes
            .iter_mut()
            .find(|s| s.node == i)
            .is_some_and(|s| std::mem::replace(&mut s.opened, false))
    }

    /// Node `i` is being freed: if it's a scope, it's forgotten, and its recorded node kept for
    /// the removal. Its subtree is freed first, so the outermost scope's is the one kept.
    pub(crate) fn forget_scope(&mut self, i: u32) {
        if let Some(k) = self.scopes.iter().position(|s| s.node == i) {
            let s = self.scopes.remove(k);
            self.scope_closed.freed = true;
            self.scope_closed.freed_back = s.back;
        }
    }

    /// A removal under `parent` freed a scope; `focus` was focused before it. If focus went with
    /// it, it comes back at the end of the build ([`Ui::give_focus_back`]).
    pub(crate) fn scope_removed(&mut self, parent: u32, focus: u32) {
        let closed = &mut self.scope_closed;
        closed.freed = false;
        let back = closed.freed_back.take();
        if focus != NIL && self.focus == NIL {
            self.scope_closed.pending = Some(GiveBack {
                back,
                parent: self.id(parent),
            });
        }
    }

    /// The end of a build: focus a scope took with it goes back to the node focused when the
    /// scope appeared, if it's still there; else to the first place Tab lands in the scope's
    /// parent. Focus moved since, to a node still there, stays.
    pub(crate) fn give_focus_back(&mut self) {
        let Some(GiveBack { back, parent }) = self.scope_closed.pending.take() else {
            return;
        };
        if self.focus != NIL {
            return;
        }
        let back = back.and_then(|id| self.index(id)).filter(|&i| {
            let n = &self.nodes[i as usize];
            n.interactive() || self.lone_viewport(i)
        });
        let to = match back {
            Some(i) => i,
            None => parent
                .and_then(|id| self.index(id))
                .map_or(NIL, |p| self.first_stop_below(p)),
        };
        if to != NIL {
            self.move_focus(to);
        }
    }

    /// The first place Tab lands among `p`'s descendants, in tree order; [`NIL`] if none.
    pub(crate) fn first_stop_below(&self, p: u32) -> u32 {
        let mut at = self.nodes[p as usize].first;
        while at != NIL {
            let stop = self.tab_stop(at, NIL, true);
            if stop != NIL {
                return stop;
            }
            let n = &self.nodes[at as usize];
            if n.first != NIL {
                at = n.first;
                continue;
            }
            // Up to the nearest ancestor below `p` with a next sibling.
            loop {
                let n = &self.nodes[at as usize];
                if n.next != NIL {
                    at = n.next;
                    break;
                }
                at = n.parent;
                if at == p || at == NIL {
                    return NIL;
                }
            }
        }
        NIL
    }

    /// An Esc nothing used: what the nearest scope holding the focus sends, or with focus
    /// outside every scope, the window's open scope's (the last opened with an escape). Returns
    /// whether a scope took it.
    pub(crate) fn scope_escape(&mut self, out: &mut Vec<M>) -> bool {
        if self.scopes.is_empty() {
            return false;
        }
        let mut inside = false;
        let mut at = self.focus;
        while at != NIL {
            if let Some(s) = self.scopes.iter().find(|s| s.node == at) {
                inside = true;
                if let Some(f) = s.on_escape {
                    out.push(f());
                    return true;
                }
            }
            at = self.nodes[at as usize].parent;
        }
        if inside {
            return false;
        }
        match self.scopes.iter().rev().find_map(|s| s.on_escape) {
            Some(f) => {
                out.push(f());
                true
            }
            None => false,
        }
    }
}
