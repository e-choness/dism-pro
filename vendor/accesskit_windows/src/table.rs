// dpro: DISM Pro's addition to the vendored adapter (PATCHES.md, 4), under the crate's
// licences: the MIT license or the Apache License, Version 2.0, at your option.

//! The parts of a table that UI Automation's Grid, Table, GridItem and TableItem patterns read,
//! found in the filtered tree, as the client sees it.
//!
//! - A table is a `Table`, `Grid` or `TreeGrid` with a row count and a column count. AccessKit
//!   computes neither, so a table without them has no Grid pattern.
//! - Its rows are `Row`s at any depth below it (inside row groups too), each with a `row_index`.
//! - Its cells are a row's `Cell`, `GridCell`, `RowHeader` and `ColumnHeader` descendants with a
//!   `column_index`, at the row's index unless they carry their own.
//! - A column header outside the rows has no row, so it isn't a cell: it's one of the table's
//!   header items, which UI Automation's HeaderItem control type keeps out of the grid.
//! - A nested table's parts are its own.
//!
//! Every lookup walks the realized tree, which a virtualized table keeps small: AccessKit nodes
//! exist only for the rows the app has built.

use accesskit::Role;
use accesskit_consumer::NodeRef;

use crate::filters::filter;

/// What [`walk`] does after visiting a node.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Walk {
    /// Visits its children next.
    Into,
    /// Goes on to its next sibling.
    Over,
    /// Ends the walk.
    Stop,
}

pub(crate) fn is_table_role(role: Role) -> bool {
    matches!(role, Role::Table | Role::Grid | Role::TreeGrid)
}

fn is_cell_role(role: Role) -> bool {
    matches!(
        role,
        Role::Cell | Role::GridCell | Role::RowHeader | Role::ColumnHeader
    )
}

/// Whether `node` is a table: a table role with both counts.
pub(crate) fn is_table(node: &NodeRef) -> bool {
    is_table_role(node.role())
        && node.data().row_count().is_some()
        && node.data().column_count().is_some()
}

/// Visits `node`'s filtered descendants in order, skipping nested tables. False if `f` stopped
/// the walk.
pub(crate) fn walk<'a>(node: &NodeRef<'a>, f: &mut impl FnMut(NodeRef<'a>) -> Walk) -> bool {
    for child in node.filtered_children(filter) {
        if is_table_role(child.role()) {
            continue;
        }
        match f(child) {
            Walk::Into => {
                if !walk(&child, f) {
                    return false;
                }
            }
            Walk::Over => {}
            Walk::Stop => return false,
        }
    }
    true
}

/// The table `node` is part of: its nearest filtered ancestor with a table role, if that one is
/// a table.
pub(crate) fn containing_table<'a>(node: &NodeRef<'a>) -> Option<NodeRef<'a>> {
    let mut at = node.filtered_parent(&filter)?;
    while !is_table_role(at.role()) {
        at = at.filtered_parent(&filter)?;
    }
    is_table(&at).then_some(at)
}

/// The row `node` is in: its nearest filtered `Row` ancestor below a table role.
fn row_of<'a>(node: &NodeRef<'a>) -> Option<NodeRef<'a>> {
    let mut at = node.filtered_parent(&filter)?;
    loop {
        match at.role() {
            Role::Row => return Some(at),
            role if is_table_role(role) => return None,
            _ => at = at.filtered_parent(&filter)?,
        }
    }
}

/// Cell `node`'s row index: its own, else its row's.
pub(crate) fn row_index(node: &NodeRef) -> Option<usize> {
    node.data()
        .row_index()
        .or_else(|| row_of(node)?.data().row_index())
}

/// Whether `node` is one of a table's cells: a cell role in a table, with a row and a column.
pub(crate) fn is_cell(node: &NodeRef) -> bool {
    is_cell_role(node.role())
        && node.data().column_index().is_some()
        && row_index(node).is_some()
        && containing_table(node).is_some()
}

/// Whether `node` is a table's header: a row group whose first child is a column header that
/// isn't a cell (UI Automation's Header control type).
pub(crate) fn is_header(node: &NodeRef) -> bool {
    node.role() == Role::RowGroup
        && node
            .filtered_children(filter)
            .next()
            .is_some_and(|first| first.role() == Role::ColumnHeader && !is_cell(&first))
}

/// The cell at `row` and `column` of `table`, if its row is in the tree.
pub(crate) fn cell_at<'a>(table: &NodeRef<'a>, row: usize, column: usize) -> Option<NodeRef<'a>> {
    let mut found = None;
    walk(table, &mut |node| match node.role() {
        Role::Row if node.data().row_index() == Some(row) => {
            walk(&node, &mut |part| match part.role() {
                role if is_cell_role(role) && part.data().column_index() == Some(column) => {
                    found = Some(part);
                    Walk::Stop
                }
                Role::Row => Walk::Over,
                _ => Walk::Into,
            });
            // Row indices are unique: this row was the one.
            Walk::Stop
        }
        Role::Row => Walk::Over,
        _ => Walk::Into,
    });
    found
}

/// `table`'s column headers in order, or only those of `column`.
pub(crate) fn column_headers<'a>(table: &NodeRef<'a>, column: Option<usize>) -> Vec<NodeRef<'a>> {
    let mut headers = Vec::new();
    walk(table, &mut |node| {
        if node.role() == Role::ColumnHeader {
            if column.is_none() || node.data().column_index() == column {
                headers.push(node);
            }
            Walk::Over
        } else {
            Walk::Into
        }
    });
    headers
}

/// The row headers of cell `node`'s row, other than `node`.
pub(crate) fn row_headers<'a>(node: &NodeRef<'a>) -> Vec<NodeRef<'a>> {
    let mut headers = Vec::new();
    if let Some(row) = row_of(node) {
        walk(&row, &mut |part| match part.role() {
            Role::RowHeader => {
                if part.id() != node.id() {
                    headers.push(part);
                }
                Walk::Over
            }
            Role::Row => Walk::Over,
            _ => Walk::Into,
        });
    }
    headers
}

/// `table`'s selected rows and grid cells, the parts that have the SelectionItem pattern.
pub(crate) fn selected<'a>(table: &NodeRef<'a>) -> Vec<NodeRef<'a>> {
    let mut selected = Vec::new();
    walk(table, &mut |node| {
        if matches!(node.role(), Role::Row | Role::GridCell) && node.is_selected() == Some(true) {
            selected.push(node);
        }
        Walk::Into
    });
    selected
}
