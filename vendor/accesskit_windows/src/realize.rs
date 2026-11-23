// dpro: DISM Pro's addition to the vendored adapter (PATCHES.md, 6), under the crate's
// licences: the MIT license or the Apache License, Version 2.0, at your option.

//! Realizing a virtualized table's rows when a client asks for one the app hasn't built.
//!
//! A virtualized table builds only the rows near its view, so AccessKit's tree holds those
//! alone. Grid's `GetItem` and ItemContainer's `FindItemByProperty` can ask for any row. When the
//! row isn't in the tree, the provider releases the tree's lock and asks the app's [`Realizer`]
//! to build it (scrolled into view), then looks again. The app answers on its own thread and
//! pushes its tree update before it returns, so the second look finds the row.

use accesskit::{ActionHandler, ActionRequest, NodeId, TreeId};
use std::sync::Mutex;

use crate::context::ActionHandlerNoMut;

/// What a provider asks the app for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Realize {
    /// Build row `row` of the table `table` (an index in its view's order) and scroll it into
    /// view.
    Row {
        tree: TreeId,
        table: NodeId,
        row: usize,
    },
    /// Find the table's first row after `after` (`None`: from its first row) whose name is
    /// `name`, in its view's order, and build it and scroll it into view.
    Named {
        tree: TreeId,
        table: NodeId,
        after: Option<usize>,
        name: String,
    },
}

/// What the app did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Realized {
    /// The row at this index is built, and the tree update that holds it was pushed.
    Row(usize),
    /// No row matches: a [`Realize::Named`] search found none, or the row is past the table's
    /// end now.
    NotFound,
    /// The app couldn't be asked: the call came on its own thread, it didn't answer in time, or
    /// it has no realizer. The provider tells the client the element isn't available, and holds
    /// nothing.
    Unavailable,
}

/// The app's side: called on UI Automation's threads with no lock of the adapter's held. It
/// returns once the app has answered, and the app pushes its tree update before it answers.
pub trait Realizer: Send + Sync {
    fn realize(&self, request: &Realize) -> Realized;
}

/// The action handler and the realizer, as the adapter's context holds them.
pub(crate) struct RealizingHandler<H: ActionHandler + Send, R: Realizer> {
    handler: Mutex<H>,
    realizer: R,
}

impl<H: ActionHandler + Send, R: Realizer> RealizingHandler<H, R> {
    pub(crate) fn new(handler: H, realizer: R) -> Self {
        Self {
            handler: Mutex::new(handler),
            realizer,
        }
    }
}

impl<H: ActionHandler + Send, R: Realizer> ActionHandlerNoMut for RealizingHandler<H, R> {
    fn do_action(&self, request: ActionRequest) {
        self.handler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .do_action(request);
    }

    // Not under the action handler's lock: a realize waits for the app, and a client's actions
    // shouldn't wait with it.
    fn realize(&self, request: &Realize) -> Realized {
        self.realizer.realize(request)
    }
}
