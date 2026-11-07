// dpro: DISM Pro's tests of its table and scroll patterns (PATCHES.md, 4), under the crate's
// licences: the MIT license or the Apache License, Version 2.0, at your option.

//! The patterns as a client reaches them, called on the providers in process: no window and no
//! UI Automation core, so they run under Wine (T2) as well as on Windows (T3).
//!
//! The tree is DISM Pro's table as dpro-ui builds it: a grid of 3 rows and 3 columns with a check
//! column, its header a row group of column headers, its rows in a viewport the client doesn't
//! see, and row 2 not built. Row 0's cells carry their row index, as dpro-ui's do; row 1's don't,
//! so theirs comes from the row, as in a web page's grid.

use accesskit::{
    Action, ActionData, ActionHandler, ActionRequest, Node, NodeId, Point, Rect, Role, ScrollUnit,
    SortDirection, Toggled, TreeId, TreeInfo, TreeUpdate,
};
use accesskit_consumer::Tree;
use std::{
    ffi::c_void,
    sync::{Arc, Mutex},
};
use windows::{
    Win32::{
        Foundation::{E_INVALIDARG, HWND},
        System::{
            Com::SAFEARRAY,
            Ole::*,
            Variant::{VARIANT, VT_BOOL, VT_BSTR, VT_I4},
        },
        UI::Accessibility::*,
    },
    core::*,
};

use crate::context::{ActionHandlerWrapper, Context};

const WINDOW: NodeId = NodeId(0);
const GRID: NodeId = NodeId(1);
const HEADER: NodeId = NodeId(2);
const BODY: NodeId = NodeId(3);
const CHECK_HEADER: NodeId = NodeId(10);
const NAME_HEADER: NodeId = NodeId(11);
const SIZE_HEADER: NodeId = NodeId(12);
const ROW_0: NodeId = NodeId(20);
const ROW_1: NodeId = NodeId(21);
const CELLS: [[NodeId; 3]; 2] = [
    [NodeId(30), NodeId(31), NodeId(32)],
    [NodeId(33), NodeId(34), NodeId(35)],
];

fn header(label: &str, column: usize) -> Node {
    let mut node = Node::new(Role::ColumnHeader);
    node.set_label(label);
    node.set_column_index(column);
    node.add_action(Action::Focus);
    node
}

fn row(at: usize, label: &str, selected: bool, cells: [NodeId; 3]) -> Node {
    let mut node = Node::new(Role::Row);
    node.set_label(label);
    node.set_row_index(at);
    node.set_selected(selected);
    node.set_children(cells.to_vec());
    node.add_action(Action::Click);
    node.add_action(Action::Focus);
    node
}

fn cell(role: Role, text: &str, row: Option<usize>, column: usize) -> Node {
    let mut node = Node::new(role);
    if !text.is_empty() {
        node.set_label(text);
    }
    if let Some(row) = row {
        node.set_row_index(row);
    }
    node.set_column_index(column);
    node
}

fn tree() -> TreeUpdate {
    let mut window = Node::new(Role::Window);
    window.set_children(vec![GRID]);
    let mut grid = Node::new(Role::Grid);
    grid.set_label("Programs");
    grid.set_row_count(3);
    grid.set_column_count(3);
    grid.set_children(vec![HEADER, BODY]);
    grid.set_bounds(Rect::new(0.0, 0.0, 300.0, 108.0));
    grid.set_scroll_y(36.0);
    grid.set_scroll_y_min(0.0);
    grid.set_scroll_y_max(72.0);
    for action in [
        Action::ScrollUp,
        Action::ScrollDown,
        Action::SetScrollOffset,
    ] {
        grid.add_action(action);
    }
    let mut head = Node::new(Role::RowGroup);
    head.set_children(vec![CHECK_HEADER, NAME_HEADER, SIZE_HEADER]);
    let mut check_header = header("Select", 0);
    check_header.set_toggled(Toggled::False);
    let mut name_header = header("Name", 1);
    name_header.set_sort_direction(SortDirection::Ascending);
    name_header.set_state_description("Sorted ascending");
    name_header.add_action(Action::Click);
    let size_header = header("Size", 2);
    let mut body = Node::new(Role::GenericContainer);
    body.set_children(vec![ROW_0, ROW_1]);
    let [c0, c1] = CELLS;
    TreeUpdate {
        nodes: vec![
            (WINDOW, window),
            (GRID, grid),
            (HEADER, head),
            (CHECK_HEADER, check_header),
            (NAME_HEADER, name_header),
            (SIZE_HEADER, size_header),
            (BODY, body),
            (ROW_0, row(0, "Alpha", false, c0)),
            (c0[0], cell(Role::Cell, "", Some(0), 0)),
            (c0[1], cell(Role::RowHeader, "Alpha", Some(0), 1)),
            (c0[2], cell(Role::Cell, "1.00 KB", Some(0), 2)),
            (ROW_1, row(1, "Beta", true, c1)),
            (c1[0], cell(Role::Cell, "", None, 0)),
            (c1[1], cell(Role::RowHeader, "Beta", None, 1)),
            (c1[2], cell(Role::Cell, "2.00 KB", None, 2)),
        ],
        tree: Some(TreeInfo::new(WINDOW)),
        tree_id: TreeId::ROOT,
        focus: GRID,
    }
}

/// Every request the providers send.
#[derive(Clone, Default)]
struct Requests(Arc<Mutex<Vec<ActionRequest>>>);

impl ActionHandler for Requests {
    fn do_action(&mut self, request: ActionRequest) {
        self.0.lock().unwrap().push(request);
    }
}

impl Requests {
    fn take(&self) -> Vec<(Action, Option<ActionData>, NodeId)> {
        self.0
            .lock()
            .unwrap()
            .drain(..)
            .map(|r| (r.action, r.data, r.target_node))
            .collect()
    }
}

/// The tree's context, with no window: these providers never ask for one.
fn context(requests: &Requests) -> Arc<Context> {
    Context::new(
        HWND::default().into(),
        Tree::new(tree(), true),
        Arc::new(ActionHandlerWrapper::new(requests.clone())),
        false,
    )
}

fn element(context: &Arc<Context>, id: NodeId) -> IRawElementProviderSimple {
    let full = context
        .read_tree()
        .state()
        .node_by_tree_local_id(id, TreeId::ROOT)
        .unwrap()
        .id();
    context.get_or_create_platform_node(full).into_interface()
}

fn pattern<T: Interface>(element: &IRawElementProviderSimple, id: UIA_PATTERN_ID) -> Option<T> {
    // A pattern the node doesn't have is a null provider.
    unsafe { element.GetPatternProvider(id) }
        .ok()
        .map(|p| p.cast().unwrap())
}

fn property(element: &IRawElementProviderSimple, id: UIA_PROPERTY_ID) -> VARIANT {
    unsafe { element.GetPropertyValue(id) }.unwrap()
}

/// A string property's value; `None` if it isn't a string.
fn text(value: &VARIANT) -> Option<String> {
    // SAFETY: the union holds a BSTR when the type says so.
    unsafe {
        let value = &value.Anonymous.Anonymous;
        (value.vt == VT_BSTR).then(|| value.Anonymous.bstrVal.to_string())
    }
}

/// An int property's value, read as the union holds it: Wine has no `VariantToInt32`.
fn int(value: &VARIANT) -> Option<i32> {
    // SAFETY: the union holds an i32 when the type says so.
    unsafe {
        let value = &value.Anonymous.Anonymous;
        (value.vt == VT_I4).then(|| value.Anonymous.lVal)
    }
}

/// A bool property's value.
fn flag(value: &VARIANT) -> Option<bool> {
    // SAFETY: the union holds a VARIANT_BOOL when the type says so.
    unsafe {
        let value = &value.Anonymous.Anonymous;
        (value.vt == VT_BOOL).then(|| value.Anonymous.boolVal.as_bool())
    }
}

fn name(element: &IRawElementProviderSimple) -> String {
    text(&property(element, UIA_NamePropertyId)).unwrap()
}

fn control_type(element: &IRawElementProviderSimple) -> UIA_CONTROLTYPE_ID {
    UIA_CONTROLTYPE_ID(int(&property(element, UIA_ControlTypePropertyId)).unwrap())
}

fn is_content(element: &IRawElementProviderSimple) -> bool {
    flag(&property(element, UIA_IsContentElementPropertyId)).unwrap()
}

/// A SAFEARRAY of providers, taken.
fn elements(array: *mut SAFEARRAY) -> Vec<IRawElementProviderSimple> {
    let mut out = Vec::new();
    if array.is_null() {
        return out;
    }
    unsafe {
        let (lower, upper) = (
            SafeArrayGetLBound(array, 1).unwrap(),
            SafeArrayGetUBound(array, 1).unwrap(),
        );
        for i in lower..=upper {
            let mut item: Option<IUnknown> = None;
            SafeArrayGetElement(array, &i, (&raw mut item).cast::<c_void>()).unwrap();
            out.push(item.unwrap().cast().unwrap());
        }
        SafeArrayDestroy(array).unwrap();
    }
    out
}

fn names(array: *mut SAFEARRAY) -> Vec<String> {
    elements(array).iter().map(name).collect()
}

#[test]
fn a_grid_counts_its_rows_and_finds_a_cell_by_row_and_column() {
    let requests = Requests::default();
    let context = context(&requests);
    let grid_element = element(&context, GRID);
    assert_eq!(control_type(&grid_element), UIA_DataGridControlTypeId);
    let grid: IGridProvider = pattern(&grid_element, UIA_GridPatternId).expect("a grid");
    assert_eq!(unsafe { grid.RowCount() }.unwrap(), 3);
    assert_eq!(unsafe { grid.ColumnCount() }.unwrap(), 3);

    // Row 1's cells take its index from the row; row 0's carry their own.
    for (row, column, text) in [(1, 1, "Beta"), (1, 2, "2.00 KB"), (0, 2, "1.00 KB")] {
        let cell = unsafe { grid.GetItem(row, column) }.unwrap();
        assert_eq!(name(&cell), text);
        assert_eq!(control_type(&cell), UIA_DataItemControlTypeId);
        let item: IGridItemProvider = pattern(&cell, UIA_GridItemPatternId).expect("a grid item");
        unsafe {
            assert_eq!((item.Row().unwrap(), item.Column().unwrap()), (row, column));
            assert_eq!(
                (item.RowSpan().unwrap(), item.ColumnSpan().unwrap()),
                (1, 1)
            );
            assert_eq!(name(&item.ContainingGrid().unwrap()), "Programs");
        }
    }

    // Row 2 is in range but not built; the others are past the grid.
    let missing = unsafe { grid.GetItem(2, 0) }.unwrap_err();
    assert_eq!(missing.code(), HRESULT(UIA_E_ELEMENTNOTAVAILABLE as i32));
    for (row, column) in [(3, 0), (0, 3), (-1, 0), (0, -1)] {
        let past = unsafe { grid.GetItem(row, column) }.unwrap_err();
        assert_eq!(past.code(), E_INVALIDARG, "({row}, {column})");
    }
}

#[test]
fn a_grid_s_header_is_a_header_of_header_items_that_aren_t_its_cells() {
    let requests = Requests::default();
    let context = context(&requests);
    let grid_element = element(&context, GRID);
    let table: ITableProvider = pattern(&grid_element, UIA_TablePatternId).expect("a table");
    unsafe {
        assert_eq!(table.RowOrColumnMajor().unwrap(), RowOrColumnMajor_RowMajor);
        assert_eq!(
            names(table.GetColumnHeaders().unwrap()),
            ["Select", "Name", "Size"]
        );
        assert!(elements(table.GetRowHeaders().unwrap()).is_empty());
    }

    let head = element(&context, HEADER);
    assert_eq!(control_type(&head), UIA_HeaderControlTypeId);
    assert!(!is_content(&head));
    let name_header = element(&context, NAME_HEADER);
    assert_eq!(control_type(&name_header), UIA_HeaderItemControlTypeId);
    assert!(!is_content(&name_header));
    assert!(pattern::<IGridItemProvider>(&name_header, UIA_GridItemPatternId).is_none());
    assert!(pattern::<ITableItemProvider>(&name_header, UIA_TableItemPatternId).is_none());
    assert!(pattern::<IInvokeProvider>(&name_header, UIA_InvokePatternId).is_some());

    // The sort reads in the app's words; an unsorted column has no status.
    let status = text(&property(&name_header, UIA_ItemStatusPropertyId));
    assert_eq!(status.as_deref(), Some("Sorted ascending"));
    let size_header = element(&context, SIZE_HEADER);
    assert!(property(&size_header, UIA_ItemStatusPropertyId).is_empty());

    // Rows and cells stay content.
    assert!(is_content(&element(&context, ROW_0)));
    assert!(is_content(&element(&context, CELLS[0][2])));
}

#[test]
fn a_cell_names_its_row_s_header_and_its_column_s() {
    let requests = Requests::default();
    let context = context(&requests);
    for (id, row_headers, column_headers) in [
        (CELLS[1][2], vec!["Beta"], vec!["Size"]),
        (CELLS[0][0], vec!["Alpha"], vec!["Select"]),
        // A row header isn't its own.
        (CELLS[1][1], vec![], vec!["Name"]),
    ] {
        let item: ITableItemProvider =
            pattern(&element(&context, id), UIA_TableItemPatternId).expect("a table item");
        unsafe {
            assert_eq!(names(item.GetRowHeaderItems().unwrap()), row_headers);
            assert_eq!(names(item.GetColumnHeaderItems().unwrap()), column_headers);
        }
    }
}

#[test]
fn a_grid_s_rows_are_selection_items_of_one_selection() {
    let requests = Requests::default();
    let context = context(&requests);
    let grid_element = element(&context, GRID);
    let selection: ISelectionProvider =
        pattern(&grid_element, UIA_SelectionPatternId).expect("a selection");
    unsafe {
        assert_eq!(names(selection.GetSelection().unwrap()), ["Beta"]);
        assert!(!selection.CanSelectMultiple().unwrap().as_bool());
    }
    let [first, second] = [ROW_0, ROW_1].map(|id| {
        pattern::<ISelectionItemProvider>(&element(&context, id), UIA_SelectionItemPatternId)
            .expect("a selection item")
    });
    unsafe {
        assert!(!first.IsSelected().unwrap().as_bool());
        assert!(second.IsSelected().unwrap().as_bool());
        assert_eq!(name(&first.SelectionContainer().unwrap()), "Programs");

        // Selecting a row clicks it, which makes it the current row.
        first.Select().unwrap();
        assert_eq!(requests.take(), [(Action::Click, None, ROW_0)]);
        // The selected row is already selected; one grid row is selected at a time; and a
        // click never deselects.
        second.Select().unwrap();
        assert_eq!(
            first.AddToSelection().unwrap_err().code(),
            HRESULT(UIA_E_INVALIDOPERATION as i32)
        );
        assert_eq!(
            second.RemoveFromSelection().unwrap_err().code(),
            HRESULT(UIA_E_INVALIDOPERATION as i32)
        );
        assert_eq!(requests.take(), []);
    }
}

#[test]
fn a_grid_with_offsets_scrolls_through_the_app_s_actions() {
    let requests = Requests::default();
    let context = context(&requests);
    let scroll: IScrollProvider =
        pattern(&element(&context, GRID), UIA_ScrollPatternId).expect("a scroll");
    unsafe {
        assert!(scroll.VerticallyScrollable().unwrap().as_bool());
        assert!(!scroll.HorizontallyScrollable().unwrap().as_bool());
        assert_eq!(scroll.VerticalScrollPercent().unwrap(), 50.0);
        assert_eq!(
            scroll.HorizontalScrollPercent().unwrap(),
            UIA_ScrollPatternNoScroll
        );
        // 108 px of 180: the view and the 72 px it can scroll.
        assert_eq!(scroll.VerticalViewSize().unwrap(), 60.0);
        assert_eq!(scroll.HorizontalViewSize().unwrap(), 100.0);

        scroll
            .Scroll(ScrollAmount_NoAmount, ScrollAmount_LargeIncrement)
            .unwrap();
        scroll
            .Scroll(ScrollAmount_NoAmount, ScrollAmount_SmallDecrement)
            .unwrap();
        scroll
            .SetScrollPercent(UIA_ScrollPatternNoScroll, 100.0)
            .unwrap();
        scroll
            .SetScrollPercent(UIA_ScrollPatternNoScroll, UIA_ScrollPatternNoScroll)
            .unwrap();
        assert_eq!(
            requests.take(),
            [
                (
                    Action::ScrollDown,
                    Some(ActionData::ScrollUnit(ScrollUnit::Page)),
                    GRID
                ),
                (
                    Action::ScrollUp,
                    Some(ActionData::ScrollUnit(ScrollUnit::Item)),
                    GRID
                ),
                (
                    Action::SetScrollOffset,
                    Some(ActionData::SetScrollOffset(Point::new(0.0, 72.0))),
                    GRID
                ),
            ]
        );

        // An axis that doesn't scroll, and a percentage past the range, fail the whole call.
        let invalid_operation = HRESULT(UIA_E_INVALIDOPERATION as i32);
        let fails = [
            scroll.Scroll(ScrollAmount_SmallIncrement, ScrollAmount_SmallIncrement),
            scroll.SetScrollPercent(50.0, UIA_ScrollPatternNoScroll),
        ];
        for result in fails {
            assert_eq!(result.unwrap_err().code(), invalid_operation);
        }
        for percent in [100.5, -0.5, f64::NAN] {
            let result = scroll.SetScrollPercent(UIA_ScrollPatternNoScroll, percent);
            assert_eq!(result.unwrap_err().code(), E_INVALIDARG, "{percent}");
        }
        assert_eq!(requests.take(), []);
    }
}
