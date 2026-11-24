//! The in-app path picker (TD-0009 A3.5 (12), A3.8, A3.9, A3.7's U19), on `fixtures/picker.rs`'s
//! page: a sheet over the page, a group named by its title; opening it moves focus to the path
//! field; Esc, Close and Cancel close it and focus goes back to Browse…; Up, Alt+Up and Backspace
//! in the table go up; Enter on a folder opens it; Enter in the path field goes there or reports
//! that it doesn't exist, isn't a folder, or isn't a full path; a listing that never answers
//! leaves the picker answering keys, Up and Cancel; the first batch paints the first screen; and a
//! picker at rest asks for nothing.

#![allow(missing_docs)]
#![expect(
    clippy::expect_used,
    reason = "test helpers: a part that isn't there fails the test"
)]

#[path = "../benches/fixtures/picker.rs"]
mod picker;

use dpro_app::picker::{BATCH, ListError, Listing};
use dpro_ui::{
    Insert, KeyCode, Mods, NodeId, Role, TABLE_BODY, TextEdit,
    accesskit::Role as AkRole,
    composite::{
        ERROR_TITLE, PICKER_CANCEL, PICKER_CLOSE, PICKER_DRIVES, PICKER_ERROR, PICKER_PATH,
        PICKER_SELECT, PICKER_TABLE, PICKER_TITLE, PICKER_UP,
    },
};
use picker::{
    CANCEL, CLOSE, DRIVES_LABEL, ENTRIES, LOADING, Model, Msg, PATH_LABEL, REGION, Run, SELECT,
    Step, TITLE, UP_LABEL, entry_name, key, root, start,
};

const NONE: Mods = Mods {
    shift: false,
    ctrl: false,
    alt: false,
    win: false,
};

const ALT: Mods = Mods {
    shift: false,
    ctrl: false,
    alt: true,
    win: false,
};

/// Opens the picker as a keyboard user does: focus on Browse…, then Enter.
fn open(run: &mut Run) {
    let opener = run.opener().expect("Browse…");
    run.ui.set_focus(Some(opener));
    run.step(key(KeyCode::Enter, NONE));
    assert!(run.model.open, "Enter on Browse… opens the picker");
    assert!(run.picker().is_some());
}

/// Opens the picker and lists its folder.
fn open_listed(run: &mut Run) {
    open(run);
    run.list_all();
    assert_eq!(run.model.folder.listing(), Listing::Listed);
}

/// Replaces the path field's text with `text`, as a paste over all of it does.
fn type_path(run: &mut Run, text: &str) {
    let end = u32::try_from(run.model.folder.path().len()).expect("a short path");
    run.step(Step::Update(Msg::Path(TextEdit {
        start: 0,
        end,
        insert: Insert::Text(text.into()),
    })));
}

/// Enter in the path field.
fn enter_in_path(run: &mut Run) {
    let field = run.part(PICKER_PATH).expect("the path field");
    run.ui.set_focus(Some(field));
    run.step(key(KeyCode::Enter, NONE));
}

/// Makes row `at` current and moves focus to the rows as a keyboard user does: opening put it in
/// the path field, and Tab from there reaches the table, whose stop is the current row.
fn focus_row(run: &mut Run, at: u32) {
    run.step(Step::Update(Msg::Table(dpro_ui::TableEvent::Current(at))));
    let field = run.part(PICKER_PATH).expect("the path field");
    run.ui.set_focus(Some(field));
    let table = run.part(PICKER_TABLE).expect("the rows");
    let on_a_row = |run: &Run| {
        run.ui.focus().is_some_and(|f| {
            run.ui.info(f).is_some_and(|n| n.role == Role::Row) && run.is_under(f, table)
        })
    };
    for _ in 0..4 {
        if on_a_row(run) {
            break;
        }
        run.step(key(KeyCode::Tab, NONE));
    }
    let focus = run.ui.focus().and_then(|f| run.ui.info(f)).map(|n| n.role);
    assert!(on_a_row(run), "Tab never reached the rows: focus on {focus:?}");
}

/// The text of the first node keyed `key` under `part`.
fn text_under(run: &Run, part: NodeId, key: dpro_ui::Key) -> String {
    let mut queue = std::collections::VecDeque::from([part]);
    while let Some(id) = queue.pop_front() {
        for c in run.ui.children(id) {
            if let Some(n) = run.ui.info(c)
                && n.key == key
            {
                return n.text.to_owned();
            }
            queue.push_back(c);
        }
    }
    String::new()
}

/// The rows built under the table's viewport.
fn built_rows(run: &Run) -> usize {
    run.part(PICKER_TABLE)
        .and_then(|t| {
            run.ui
                .children(t)
                .find(|&c| run.ui.info(c).is_some_and(|n| n.key == TABLE_BODY))
        })
        .map_or(0, |body| {
            run.ui
                .children(body)
                .filter(|&c| run.ui.info(c).is_some_and(|n| n.role == Role::Row))
                .count()
        })
}

#[test]
fn a_client_sees_a_group_named_by_its_title_and_its_parts() {
    let mut run = Run::accessible(Model::new());
    run.step(Step::Update(Msg::Open));
    run.list_all();
    let tree = run.ui.a11y_tree();
    let ak = |id: Option<NodeId>| id.and_then(|id| run.ak(&tree, id));
    let sheet = ak(run.picker()).expect("the picker");
    // UI Automation's Group either way: a region is a group with a landmark type.
    assert!(
        matches!(sheet.role(), AkRole::Group | AkRole::Region),
        "{:?}",
        sheet.role()
    );
    assert_eq!(sheet.label(), Some(TITLE));
    if sheet.role() == AkRole::Region {
        assert_eq!(sheet.role_description(), Some(REGION));
    }
    let title = ak(run.part(PICKER_TITLE)).expect("the title");
    // AccessKit's levels are aria-level less one.
    assert_eq!(title.level(), Some(1), "a level-2 heading");
    let path = ak(run.part(PICKER_PATH)).expect("the path field");
    assert_eq!(path.role(), AkRole::TextInput);
    assert_eq!(path.label(), Some(PATH_LABEL));
    assert_eq!(path.value(), Some(start().to_string_lossy().as_ref()));
    let up = ak(run.part(PICKER_UP)).expect("Up");
    assert_eq!((up.role(), up.label()), (AkRole::Button, Some(UP_LABEL)));
    assert!(up.children().is_empty(), "its glyph is presentational");
    let drives = ak(run.part(PICKER_DRIVES)).expect("the drive list");
    assert_eq!(
        (drives.role(), drives.label()),
        (AkRole::ComboBox, Some(DRIVES_LABEL))
    );
    let grid = ak(run.part(PICKER_TABLE)).expect("the rows");
    assert_eq!(grid.role(), AkRole::Grid);
    for (part, name) in [
        (PICKER_CLOSE, CLOSE),
        (PICKER_CANCEL, CANCEL),
        (PICKER_SELECT, SELECT),
    ] {
        let b = ak(run.part(part)).expect("a button");
        assert_eq!((b.role(), b.label()), (AkRole::Button, Some(name)));
    }
    // The page is out of the tree while the sheet covers it.
    assert!(run.opener().is_none());
}

#[test]
fn opening_moves_focus_to_the_path_field() {
    let mut run = Run::with(Model::new());
    open(&mut run);
    assert_eq!(run.ui.focus(), run.part(PICKER_PATH));
}

#[test]
fn esc_closes_the_sheet_and_focus_goes_back_to_browse() {
    let mut run = Run::with(Model::new());
    open_listed(&mut run);
    let focus = run.ui.focus().expect("focus is in the picker");
    assert!(run.is_under(focus, run.picker().expect("open")));
    run.step(key(KeyCode::Escape, NONE));
    assert_eq!(run.model.last, Some(Msg::Close), "Esc sends close");
    assert!(run.picker().is_none());
    // The page is built again: Browse…, its first control, is a new node.
    assert_eq!(run.ui.focus(), run.opener(), "focus is back on Browse…");
}

#[test]
fn close_and_cancel_close_it_and_focus_goes_back_too() {
    for part in [PICKER_CLOSE, PICKER_CANCEL] {
        let mut run = Run::with(Model::new());
        open_listed(&mut run);
        let button = run.part(part).expect("the button");
        run.ui.set_focus(Some(button));
        run.step(key(KeyCode::Enter, NONE));
        assert_eq!(run.model.last, Some(Msg::Close));
        assert!(run.picker().is_none());
        assert_eq!(run.ui.focus(), run.opener());
    }
}

#[test]
fn up_goes_to_the_parent_folder() {
    let mut run = Run::with(Model::new());
    open_listed(&mut run);
    let up = run.part(PICKER_UP).expect("Up");
    run.ui.set_focus(Some(up));
    run.step(key(KeyCode::Enter, NONE));
    let parent = start().parent().map(std::path::Path::to_path_buf);
    assert_eq!(run.model.folder.folder(), parent.as_deref());
    assert_eq!(
        run.model.folder.path(),
        root().join("Images").to_string_lossy()
    );
    assert_eq!(run.model.folder.listing(), Listing::Loading);
}

#[test]
fn alt_up_anywhere_and_backspace_in_the_table_go_up() {
    // Alt+Up from the path field.
    let mut run = Run::with(Model::new());
    open_listed(&mut run);
    let field = run.part(PICKER_PATH).expect("the path field");
    run.ui.set_focus(Some(field));
    run.step(key(KeyCode::Up, ALT));
    assert_eq!(run.model.last, Some(Msg::Up), "Alt+Up");
    assert_eq!(run.model.lister.started.len(), 2);
    // Backspace on a row.
    let mut run = Run::with(Model::new());
    open_listed(&mut run);
    focus_row(&mut run, 0);
    let focus = run.ui.focus().expect("a row");
    assert!(run.is_under(focus, run.part(PICKER_TABLE).expect("the rows")));
    run.step(key(KeyCode::Backspace, NONE));
    assert_eq!(run.model.last, Some(Msg::Up), "Backspace in the table");
    // In the path field, Backspace edits the path instead.
    let field = run.part(PICKER_PATH).expect("the path field");
    run.ui.set_focus(Some(field));
    run.step(key(KeyCode::End, NONE));
    let before = run.model.lister.started.len();
    run.step(key(KeyCode::Backspace, NONE));
    assert_eq!(run.model.lister.started.len(), before);
    assert!(matches!(run.model.last, Some(Msg::Path(_))));
}

#[test]
fn enter_on_a_folder_opens_it() {
    let mut run = Run::with(Model::new());
    open_listed(&mut run);
    focus_row(&mut run, 2);
    run.step(key(KeyCode::Enter, NONE));
    let opened = start().join(entry_name(2));
    assert_eq!(run.model.folder.folder(), Some(opened.as_path()));
    assert_eq!(run.model.folder.path(), opened.to_string_lossy());
    run.list_all();
    assert_eq!(run.model.folder.len(), u32::try_from(ENTRIES).unwrap_or(0));
}

#[test]
fn enter_in_the_path_field_goes_there() {
    let mut run = Run::with(Model::new());
    open_listed(&mut run);
    let other = root().join("Other");
    type_path(&mut run, &other.to_string_lossy());
    enter_in_path(&mut run);
    assert_eq!(run.model.last, Some(Msg::Go));
    assert_eq!(run.model.folder.folder(), Some(other.as_path()));
    assert_eq!(
        run.model.lister.started.last().map(|r| r.path.clone()),
        Some(other)
    );
    run.list_all();
    assert_eq!(run.model.folder.len(), u32::try_from(ENTRIES).unwrap_or(0));
}

#[test]
fn a_path_that_isn_t_there_or_isn_t_a_folder_is_reported() {
    for (name, error, says) in [
        ("missing", ListError::NotFound, "doesn't exist."),
        ("notes.txt", ListError::NotFolder, "isn't a folder."),
    ] {
        let mut run = Run::accessible(Model::new());
        open_listed(&mut run);
        let typed = root().join(name);
        type_path(&mut run, &typed.to_string_lossy());
        enter_in_path(&mut run);
        run.list_all();
        assert_eq!(run.model.folder.listing(), Listing::Failed(error));
        let panel = run.part(PICKER_ERROR).expect("the error panel");
        let title = text_under(&run, panel, ERROR_TITLE);
        assert_eq!(title, format!("“{}” {says}", typed.to_string_lossy()));
        // The field keeps what was typed, for the user to fix.
        assert_eq!(run.model.folder.path(), typed.to_string_lossy());
        assert!(run.picker().is_some(), "{name}: the picker stays open");
    }
}

#[test]
fn dot_dot_is_refused_before_anything_is_listed() {
    let mut run = Run::with(Model::new());
    open_listed(&mut run);
    let before = run.model.lister.started.len();
    let typed = start().join("..").join("Other");
    type_path(&mut run, &typed.to_string_lossy());
    enter_in_path(&mut run);
    assert_eq!(run.model.lister.started.len(), before, "nothing listed");
    assert!(run.model.refused.is_some());
    assert!(run.part(PICKER_ERROR).is_some(), "the panel says why");
    assert_eq!(run.model.folder.folder(), Some(start().as_path()));
    assert_eq!(run.model.folder.len(), u32::try_from(ENTRIES).unwrap_or(0));
}

/// A3.8: a folder that never answers shows `ui-loading`, and Cancel still closes the picker.
#[test]
fn cancel_closes_the_picker_while_a_listing_hangs() {
    let mut model = Model::new();
    model.lister.hang = true;
    let mut run = Run::with(model);
    open(&mut run);
    run.list_all();
    assert_eq!(run.model.folder.listing(), Listing::Loading);
    let table = run.part(PICKER_TABLE).expect("the rows");
    assert!(
        run.ui
            .children(table)
            .flat_map(|c| run.ui.children(c).collect::<Vec<_>>())
            .any(|c| run.ui.info(c).is_some_and(|n| n.text == LOADING)),
        "the status line says it's loading"
    );
    let cancel = run.part(PICKER_CANCEL).expect("Cancel");
    run.ui.set_focus(Some(cancel));
    run.step(key(KeyCode::Enter, NONE));
    assert!(run.picker().is_none(), "Cancel answered");
}

/// A3.7 U19's exact T1 test: while a listing never answers, the picker answers keys and Up, each
/// a new listing at once; the UI thread never waits.
#[test]
fn up_and_keys_answer_while_a_listing_hangs() {
    let mut model = Model::new();
    model.lister.hang = true;
    let mut run = Run::with(model);
    open(&mut run);
    run.list_all();
    assert_eq!(run.model.folder.listing(), Listing::Loading);
    run.step(key(KeyCode::Up, ALT));
    assert_eq!(
        run.model.lister.started.len(),
        2,
        "Alt+Up listed the parent"
    );
    assert_eq!(
        run.model.folder.folder(),
        start().parent(),
        "and the field shows it"
    );
    let up = run.part(PICKER_UP).expect("Up");
    run.ui.set_focus(Some(up));
    run.step(key(KeyCode::Enter, NONE));
    assert_eq!(run.model.lister.started.len(), 3, "Up listed the root");
    // Tab still moves focus through the sheet.
    let before = run.ui.focus();
    run.step(key(KeyCode::Tab, NONE));
    assert_ne!(run.ui.focus(), before);
    run.step(key(KeyCode::Escape, NONE));
    assert!(run.picker().is_none(), "Esc answered");
}

/// U19's first screen: the first batch's rows are built, as many as the view shows (and its
/// half-views either side), not the batch's 512.
#[test]
fn the_first_batch_paints_the_first_screen_of_rows() {
    let mut run = Run::waiting(Model::new(), None);
    assert_eq!(built_rows(&run), 0, "nothing came yet");
    run.step(Step::Update(Msg::Listed));
    assert_eq!(run.model.folder.len(), u32::try_from(BATCH).unwrap_or(0));
    let rows = built_rows(&run);
    assert!(rows > 0 && rows < BATCH / 4, "{rows} rows built");
}

#[test]
fn a_picker_at_rest_asks_for_nothing() {
    let mut run = Run::accessible(Model::new());
    open_listed(&mut run);
    run.settle();
    assert!(!run.ui.wants_frame(), "no frames");
    assert!(!run.ui.wants_build(), "no second build");
    // The path field's caret blinks for a while (Caret::BLINK_FOR_MS), then rests.
    let blink_rests = u64::from(dpro_ui::Caret::BLINK_FOR_MS);
    let mut waited = 0;
    while let Some(ms) = run.wake() {
        assert!(waited <= blink_rests, "still waking after {waited} ms");
        run.step(Step::Tick(ms.max(1)));
        waited += ms.max(1);
    }
    assert!(!run.ui.wants_frame(), "no frames after the caret rests");
    // A listing still loading asks for nothing either: its batches wake the window.
    let mut model = Model::new();
    model.lister.hang = true;
    let mut run = Run::accessible(model);
    open(&mut run);
    run.settle();
    assert!(!run.ui.wants_frame());
    assert!(!run.ui.wants_build());
}
