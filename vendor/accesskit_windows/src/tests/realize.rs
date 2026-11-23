// dpro: DISM Pro's tests of rows realized on request (PATCHES.md, 6), under the crate's
// licences: the MIT license or the Apache License, Version 2.0, at your option.

//! Grid's `GetItem` and ItemContainer's `FindItemByProperty` on the table of
//! [`table`](super::table), whose row 2 ("Gamma") isn't built. The app's half is played in
//! process: asked for row 2, or for a name its view holds, it builds the row and pushes the update
//! before it answers, as dpro-win's realizer does on the UI thread.

use accesskit::{NodeId, TreeId, TreeUpdate};
use accesskit_consumer::{NodeRef, TreeChangeHandler};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use windows::{
    Win32::{
        Foundation::{E_INVALIDARG, S_OK},
        System::Variant::VARIANT,
        UI::Accessibility::*,
    },
    core::*,
};

use super::{
    providers::{Requests, context_with, element, name, pattern},
    table::{self, BODY, CELLS, GRID, NAME_HEADER, ROW_0, ROW_1},
};
use crate::{Realize, Realized, Realizer, context::Context, util::Variant};

const ROW_2: NodeId = NodeId(22);
const CELLS_2: [NodeId; 3] = [NodeId(36), NodeId(37), NodeId(38)];
/// The view's rows, by name, in order.
const VIEW: [&str; 3] = ["Alpha", "Beta", "Gamma"];

/// The app: it builds row 2 when asked for it (or found by name), and remembers each request.
#[derive(Clone, Default)]
struct App {
    context: Arc<OnceLock<Weak<Context>>>,
    asked: Arc<Mutex<Vec<Realize>>>,
    /// It can't be asked: the call came on its thread, or it didn't answer in time.
    unavailable: bool,
}

struct Unheard;

impl TreeChangeHandler for Unheard {
    fn node_added(&mut self, _: &NodeRef) {}
    fn node_updated(&mut self, _: &NodeRef, _: &NodeRef) {}
    fn focus_moved(&mut self, _: Option<&NodeRef>, _: Option<&NodeRef>) {}
    fn node_removed(&mut self, _: &NodeRef) {}
}

impl App {
    fn asked(&self) -> Vec<Realize> {
        std::mem::take(&mut *self.asked.lock().unwrap())
    }

    /// Builds row `at` if it isn't, pushing the update while nothing holds the tree.
    fn build(&self, at: usize) -> Realized {
        if at == 2 {
            let context = self.context.get().and_then(Weak::upgrade).unwrap();
            let mut body = accesskit::Node::new(accesskit::Role::GenericContainer);
            body.set_children(vec![ROW_0, ROW_1, ROW_2]);
            let update = TreeUpdate {
                nodes: vec![
                    (BODY, body),
                    (ROW_2, table::row(2, "Gamma", false, CELLS_2)),
                    (
                        CELLS_2[0],
                        table::cell(accesskit::Role::Cell, "", Some(2), 0),
                    ),
                    (
                        CELLS_2[1],
                        table::cell(accesskit::Role::RowHeader, "Gamma", Some(2), 1),
                    ),
                    (
                        CELLS_2[2],
                        table::cell(accesskit::Role::Cell, "3.00 KB", Some(2), 2),
                    ),
                ],
                tree: None,
                tree_id: TreeId::ROOT,
                focus: GRID,
            };
            context
                .tree
                .write()
                .unwrap()
                .update_and_process_changes(update, &mut Unheard);
        }
        Realized::Row(at)
    }
}

impl Realizer for App {
    fn realize(&self, request: &Realize) -> Realized {
        self.asked.lock().unwrap().push(request.clone());
        if self.unavailable {
            return Realized::Unavailable;
        }
        match request {
            Realize::Row { row, .. } if *row < VIEW.len() => self.build(*row),
            Realize::Row { .. } => Realized::NotFound,
            Realize::Named { after, name, .. } => {
                let from = after.map_or(0, |at| at + 1);
                match (from..VIEW.len()).find(|&at| VIEW[at] == name) {
                    Some(at) => self.build(at),
                    None => Realized::NotFound,
                }
            }
        }
    }
}

fn context(app: &App) -> Arc<Context> {
    let context = context_with(&Requests::default(), table::tree(), app.clone());
    app.context.set(Arc::downgrade(&context)).unwrap();
    context
}

fn row(at: usize) -> Realize {
    Realize::Row {
        tree: TreeId::ROOT,
        table: GRID,
        row: at,
    }
}

fn named(after: Option<usize>, name: &str) -> Realize {
    Realize::Named {
        tree: TreeId::ROOT,
        table: GRID,
        after,
        name: name.to_owned(),
    }
}

fn container(context: &Arc<Context>) -> IItemContainerProvider {
    pattern(&element(context, GRID), UIA_ItemContainerPatternId).expect("an item container")
}

/// `FindItemByProperty`'s row's name, or `None` for a null element (S_OK).
fn find(
    container: &IItemContainerProvider,
    after: Option<&IRawElementProviderSimple>,
    property: UIA_PROPERTY_ID,
    value: Variant,
) -> Result<Option<String>> {
    let value: VARIANT = value.into();
    match unsafe { container.FindItemByProperty(after, property, &value) } {
        Ok(found) => Ok(Some(name(&found))),
        Err(e) if e.code() == S_OK => Ok(None),
        Err(e) => Err(e),
    }
}

fn found(
    container: &IItemContainerProvider,
    after: Option<&IRawElementProviderSimple>,
    property: UIA_PROPERTY_ID,
    value: Variant,
) -> IRawElementProviderSimple {
    let value: VARIANT = value.into();
    unsafe { container.FindItemByProperty(after, property, &value) }.unwrap()
}

#[test]
fn a_cell_of_a_row_not_built_is_realized_then_found() {
    let app = App::default();
    let context = context(&app);
    let grid: IGridProvider = pattern(&element(&context, GRID), UIA_GridPatternId).unwrap();
    let cell = unsafe { grid.GetItem(2, 1) }.unwrap();
    assert_eq!(name(&cell), "Gamma");
    let item: IGridItemProvider = pattern(&cell, UIA_GridItemPatternId).unwrap();
    assert_eq!(unsafe { item.Row() }.unwrap(), 2);
    assert_eq!(app.asked(), [row(2)]);

    // Built now, so found without asking; a built row never asks.
    unsafe {
        assert_eq!(name(&grid.GetItem(2, 2).unwrap()), "3.00 KB");
        assert_eq!(name(&grid.GetItem(0, 1).unwrap()), "Alpha");
    }
    assert_eq!(app.asked(), []);
}

#[test]
fn an_app_that_can_t_answer_leaves_the_cell_unavailable() {
    let app = App {
        unavailable: true,
        ..App::default()
    };
    let context = context(&app);
    let grid: IGridProvider = pattern(&element(&context, GRID), UIA_GridPatternId).unwrap();
    let missing = unsafe { grid.GetItem(2, 0) }.unwrap_err();
    assert_eq!(missing.code(), HRESULT(UIA_E_ELEMENTNOTAVAILABLE as i32));
    assert_eq!(app.asked(), [row(2)]);
    // Past the counts, the app isn't asked.
    let past = unsafe { grid.GetItem(3, 0) }.unwrap_err();
    assert_eq!(past.code(), E_INVALIDARG);
    assert_eq!(app.asked(), []);
}

#[test]
fn property_0_walks_the_rows_in_order_and_realizes_the_next() {
    let app = App::default();
    let context = context(&app);
    let container = container(&context);
    let empty = Variant::empty;
    let first = found(&container, None, UIA_PROPERTY_ID(0), empty());
    assert_eq!(name(&first), "Alpha");
    let second = found(&container, Some(&first), UIA_PROPERTY_ID(0), empty());
    assert_eq!(name(&second), "Beta");
    assert_eq!(app.asked(), []);
    let third = found(&container, Some(&second), UIA_PROPERTY_ID(0), empty());
    assert_eq!(name(&third), "Gamma");
    assert_eq!(app.asked(), [row(2)]);
    // Past the last row: a null element, and S_OK.
    assert_eq!(
        find(&container, Some(&third), UIA_PROPERTY_ID(0), empty()).unwrap(),
        None
    );
    // A cell starts the search after its row.
    let cell = element(&context, CELLS[0][2]);
    assert_eq!(
        find(&container, Some(&cell), UIA_PROPERTY_ID(0), empty()).unwrap(),
        Some("Beta".to_owned())
    );
}

#[test]
fn a_name_is_searched_for_by_the_app_which_realizes_the_row_it_finds() {
    let app = App::default();
    let context = context(&app);
    let container = container(&context);
    let by_name = |s: &str| Variant::from(BSTR::from(s));
    let gamma = found(&container, None, UIA_NamePropertyId, by_name("Gamma"));
    assert_eq!(name(&gamma), "Gamma");
    assert_eq!(app.asked(), [named(None, "Gamma")]);

    // From a row on, and a name no row after it has.
    let beta = element(&context, ROW_1);
    assert_eq!(
        find(
            &container,
            Some(&beta),
            UIA_NamePropertyId,
            by_name("Alpha")
        )
        .unwrap(),
        None
    );
    assert_eq!(app.asked(), [named(Some(1), "Alpha")]);
    assert_eq!(
        find(&container, None, UIA_NamePropertyId, by_name("alpha")).unwrap(),
        None,
        "names match exactly"
    );
}

#[test]
fn without_the_app_a_name_is_found_among_the_rows_built() {
    let app = App {
        unavailable: true,
        ..App::default()
    };
    let context = context(&app);
    let container = container(&context);
    let by_name = |s: &str| Variant::from(BSTR::from(s));
    assert_eq!(
        find(&container, None, UIA_NamePropertyId, by_name("Beta")).unwrap(),
        Some("Beta".to_owned())
    );
    assert_eq!(
        find(&container, None, UIA_NamePropertyId, by_name("Gamma")).unwrap(),
        None
    );
    assert_eq!(app.asked(), [named(None, "Beta"), named(None, "Gamma")]);
}

#[test]
fn a_selection_search_finds_the_selected_row_and_realizes_an_unselected_one() {
    let app = App::default();
    let context = context(&app);
    let container = container(&context);
    let selected = found(
        &container,
        None,
        UIA_SelectionItemIsSelectedPropertyId,
        true.into(),
    );
    assert_eq!(name(&selected), "Beta");
    assert_eq!(
        find(
            &container,
            Some(&selected),
            UIA_SelectionItemIsSelectedPropertyId,
            true.into()
        )
        .unwrap(),
        None
    );
    let unselected = found(
        &container,
        None,
        UIA_SelectionItemIsSelectedPropertyId,
        false.into(),
    );
    assert_eq!(name(&unselected), "Alpha");
    assert_eq!(app.asked(), []);
    // Past Alpha, Beta is selected: the next unselected row is 2, realized.
    let next = found(
        &container,
        Some(&unselected),
        UIA_SelectionItemIsSelectedPropertyId,
        false.into(),
    );
    assert_eq!(name(&next), "Gamma");
    assert_eq!(app.asked(), [row(2)]);
}

#[test]
fn other_properties_values_and_starts_are_invalid_arguments() {
    let app = App::default();
    let context = context(&app);
    let container = container(&context);
    let invalid = |r: Result<Option<String>>| r.unwrap_err().code();
    let text = || Variant::from(BSTR::from("Alpha"));
    assert_eq!(
        invalid(find(&container, None, UIA_AutomationIdPropertyId, text())),
        E_INVALIDARG
    );
    // A name that isn't a string, and a selection that isn't a bool.
    assert_eq!(
        invalid(find(&container, None, UIA_NamePropertyId, 7.into())),
        E_INVALIDARG
    );
    assert_eq!(
        invalid(find(
            &container,
            None,
            UIA_SelectionItemIsSelectedPropertyId,
            text()
        )),
        E_INVALIDARG
    );
    // A header item isn't one of the rows.
    let header = element(&context, NAME_HEADER);
    assert_eq!(
        invalid(find(&container, Some(&header), UIA_NamePropertyId, text())),
        E_INVALIDARG
    );
    assert_eq!(app.asked(), []);
    // Only the table is a container.
    assert!(
        pattern::<IItemContainerProvider>(&element(&context, ROW_0), UIA_ItemContainerPatternId)
            .is_none()
    );
}
