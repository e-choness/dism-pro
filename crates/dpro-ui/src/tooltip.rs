//! Tooltips (TD-0009 A1.4): which node's tooltip shows, and when, on the window's one-shot wake.
//!
//! - **Showing.** The pointer resting on a node with [tooltip text](crate::Element::tooltip)
//!   shows it after the delay (Windows' double-click time, its tooltip delay), and the keyboard
//!   focusing one shows it at once. Moving to a neighbor within a fifth of the delay of the last
//!   tip hiding shows the neighbor's at once (Windows' reshow).
//! - **Hiding.** A press, any key, the pointer leaving (for a tip the pointer started), the window
//!   going inactive, or ten delays passing (autopop). A tip hidden by a press, a key or autopop
//!   while the pointer is on its node doesn't come back until the pointer leaves it.
//! - **Time.** Input carries no time, so input only records what changed, and
//!   [`Ui::tooltip_timer`], which the platform runs after every event and on its wake, stamps it
//!   with the clock. A pending tip costs one wake and no frames (U6).
//!
//! Only the active window shows tips, so a menu over the main window shows its own and the main
//! window's stop.

use crate::{NodeId, geom::Point, theme::tokens::popup, tree::NIL, ui::Ui};

/// What started a tip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TipSource {
    Pointer,
    Focus,
}

/// Where the tip is in its life.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TipState {
    Idle,
    /// Started since the last timer, which stamps it: at once if `now`.
    Starting {
        now: bool,
    },
    /// Shows at `due`.
    Pending {
        due: u64,
    },
    /// Showing, until `until` (autopop).
    Showing {
        until: u64,
    },
}

/// The window's one tooltip.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Tip {
    /// The node whose tip is pending or showing; [`NIL`] for none.
    pub(crate) node: u32,
    pub(crate) state: TipState,
    pub(crate) source: TipSource,
    /// The node with a tip under the pointer, [`NIL`] for none.
    pub(crate) over: u32,
    /// A node whose tip was hidden by a press, a key or autopop: it doesn't start again until the
    /// pointer leaves it.
    pub(crate) spent: u32,
    /// A showing tip hid since the last timer, which stamps `hidden_at`.
    pub(crate) hid: bool,
    pub(crate) hidden_at: Option<u64>,
    /// Windows' tooltip delay, in milliseconds.
    pub(crate) delay: u32,
}

impl Tip {
    pub(crate) const fn new() -> Self {
        Self {
            node: NIL,
            state: TipState::Idle,
            source: TipSource::Pointer,
            over: NIL,
            spent: NIL,
            hid: false,
            hidden_at: None,
            delay: popup::TIP_DELAY_MS,
        }
    }
}

impl<M> Ui<M> {
    /// The tooltip showing, if any: its node, and its text.
    #[must_use]
    pub fn tooltip(&self) -> Option<(NodeId, &str)> {
        if !matches!(self.tip.state, TipState::Showing { .. }) {
            return None;
        }
        let id = self.id(self.tip.node)?;
        Some((id, self.tip_text(self.tip.node)?))
    }

    /// Windows' tooltip delay changed (its double-click time, `GetDoubleClickTime`), in
    /// milliseconds. A tip already pending keeps its time.
    pub fn set_tooltip_delay(&mut self, ms: u32) {
        self.tip.delay = ms;
    }

    /// Advances the tooltip to `now_ms` on the monotonic clock: a pending tip that's due shows,
    /// and one showing for ten delays hides. Returns how long until it needs to run again, if it
    /// does; the platform runs it after every event and when that time comes (one shot, as the
    /// caret's).
    pub fn tooltip_timer(&mut self, now_ms: u64) -> Option<u64> {
        if std::mem::take(&mut self.tip.hid) {
            self.tip.hidden_at = Some(now_ms);
        }
        let delay = u64::from(self.tip.delay);
        let shows_for = delay * u64::from(popup::TIP_SHOWS_FOR);
        match self.tip.state {
            TipState::Idle => None,
            TipState::Starting { now } => {
                let reshow = self.tip.hidden_at.is_some_and(|t| {
                    now_ms.saturating_sub(t) < delay / u64::from(popup::TIP_RESHOW_DIV)
                });
                if now || reshow || delay == 0 {
                    self.tip.state = TipState::Showing {
                        until: now_ms.saturating_add(shows_for),
                    };
                    Some(shows_for.max(1))
                } else {
                    self.tip.state = TipState::Pending {
                        due: now_ms.saturating_add(delay),
                    };
                    Some(delay)
                }
            }
            TipState::Pending { due } if now_ms >= due => {
                self.tip.state = TipState::Showing {
                    until: now_ms.saturating_add(shows_for),
                };
                Some(shows_for.max(1))
            }
            TipState::Pending { due } => Some(due - now_ms),
            TipState::Showing { until } if now_ms >= until => {
                self.tip_cancel(true);
                self.tip.hid = false;
                self.tip.hidden_at = Some(now_ms);
                None
            }
            TipState::Showing { until } => Some(until - now_ms),
        }
    }

    /// The tip of node `i`, if it has one.
    pub(crate) fn tip_text(&self, i: u32) -> Option<&str> {
        let side = self.nodes.get(i as usize)?.side;
        let tip = self.sides.get(side as usize)?.tip.as_str();
        (!tip.is_empty()).then_some(tip)
    }

    /// The pointer moved to `p`, or left (`None`): a tip under it starts, and a tip the pointer
    /// started hides when it leaves that node.
    pub(crate) fn tip_pointer(&mut self, p: Option<Point>) {
        let over = p.map_or(NIL, |p| self.tip_hit(p));
        if over == self.tip.over {
            return;
        }
        self.tip.over = over;
        if over != self.tip.spent {
            self.tip.spent = NIL;
        }
        if over == NIL {
            if self.tip.source == TipSource::Pointer {
                self.tip_cancel(false);
            }
        } else if self.active && over != self.tip.spent {
            self.tip_start(over, TipSource::Pointer);
        }
    }

    /// The keyboard moved focus to `i`: its tip shows at once, if it has one.
    pub(crate) fn tip_focus(&mut self, i: u32) {
        if self.active && i != NIL && i != self.tip.node && self.tip_text(i).is_some() {
            self.tip_start(i, TipSource::Focus);
        }
    }

    fn tip_start(&mut self, i: u32, source: TipSource) {
        self.tip_cancel(false);
        self.tip.node = i;
        self.tip.source = source;
        self.tip.state = TipState::Starting {
            now: source == TipSource::Focus,
        };
    }

    /// The tip goes, pending or showing; if `spend` and the pointer is on its node, it doesn't
    /// start again until the pointer leaves. (A tip the keyboard showed, the pointer elsewhere,
    /// may show for the pointer later.)
    pub(crate) fn tip_cancel(&mut self, spend: bool) {
        if self.tip.node == NIL {
            return;
        }
        if matches!(self.tip.state, TipState::Showing { .. }) {
            self.tip.hid = true;
        }
        if spend && self.tip.over == self.tip.node {
            self.tip.spent = self.tip.node;
        }
        self.tip.node = NIL;
        self.tip.state = TipState::Idle;
    }

    /// Node `i` lost its tip: the tip, and what refers to it, go.
    pub(crate) fn tip_forget(&mut self, i: u32) {
        if self.tip.node == i {
            self.tip_cancel(false);
        }
        if self.tip.over == i {
            self.tip.over = NIL;
        }
        if self.tip.spent == i {
            self.tip.spent = NIL;
        }
    }

    /// A subtree was just freed: a tip on one of its nodes, and what refers to them, go with it.
    pub(crate) fn tip_forget_freed(&mut self) {
        let gone = |i: u32| i != NIL && !self.nodes[i as usize].live;
        let (node, over, spent) = (
            gone(self.tip.node),
            gone(self.tip.over),
            gone(self.tip.spent),
        );
        if node {
            self.tip_cancel(false);
        }
        if over {
            self.tip.over = NIL;
        }
        if spent {
            self.tip.spent = NIL;
        }
    }

    /// The topmost node with a tip under `p`, enabled or not (a disabled command's tip says why);
    /// [`NIL`] if none.
    fn tip_hit(&self, p: Point) -> u32 {
        let mut found = NIL;
        let (_, bar) = self.walk_at(p, |at| {
            if self.nodes[at as usize].side != NIL && self.tip_text(at).is_some() {
                found = at;
            }
        });
        // A scroll bar has no tip, and hides what's under it.
        if bar.is_some() { NIL } else { found }
    }
}
