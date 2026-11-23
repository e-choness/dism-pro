// dpro: DISM Pro's tests of headings, landmarks and accelerators (PATCHES.md, planned items 8–10;
// TD-0009 A4.5), under the crate's licences: the MIT license or the Apache License, Version 2.0,
// at your option. Written before the patches, so ignored until they land.

//! Headings, landmarks and accelerator keys as a client reads them, called on the providers in
//! process.
//!
//! - A `Heading` with AccessKit level *n* (zero-based) reports HeadingLevel 80051 + *n*, and no
//!   UIA Level. Other roles keep their Level, and report HeadingLevel_None.
//! - `Main`, `Navigation` and `Search` report LandmarkType 80002, 80003 and 80004. A named
//!   `Region` reports Custom (80000) with its role description as LocalizedLandmarkType, and keeps
//!   the default LocalizedControlType. An unnamed region is no landmark.
//! - `keyboard_shortcut` is the AcceleratorKey.

use accesskit::{Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
use windows::Win32::UI::Accessibility::*;

use super::providers::{Requests, context, element, int, property, text};

const WINDOW: NodeId = NodeId(0);
const TITLE: NodeId = NodeId(1);
const SECTION: NodeId = NodeId(2);
const ITEM: NodeId = NodeId(3);
const RAIL: NodeId = NodeId(4);
const MAIN: NodeId = NodeId(5);
const SEARCH: NodeId = NodeId(6);
const DRAWER: NodeId = NodeId(7);
const UNNAMED: NodeId = NodeId(8);
const BUTTON: NodeId = NodeId(9);

fn node(role: Role, label: Option<&str>) -> Node {
    let mut node = Node::new(role);
    if let Some(label) = label {
        node.set_label(label);
    }
    node
}

fn tree() -> TreeUpdate {
    let mut window = Node::new(Role::Window);
    window.set_children(vec![
        TITLE, SECTION, ITEM, RAIL, MAIN, SEARCH, DRAWER, UNNAMED, BUTTON,
    ]);
    let mut title = node(Role::Heading, Some("Clean"));
    title.set_level(0);
    let mut section = node(Role::Heading, Some("Cleanup"));
    section.set_level(1);
    let mut item = node(Role::TreeItem, Some("Logs"));
    item.set_level(2);
    let mut drawer = node(Role::Region, Some("Changes"));
    drawer.set_role_description("region");
    let mut unnamed = Node::new(Role::Region);
    unnamed.set_role_description("region");
    let mut button = node(Role::Button, Some("Review changes"));
    button.set_keyboard_shortcut("Ctrl+Shift+R");
    TreeUpdate {
        nodes: vec![
            (WINDOW, window),
            (TITLE, title),
            (SECTION, section),
            (ITEM, item),
            (RAIL, node(Role::Navigation, Some("Pages"))),
            (MAIN, node(Role::Main, Some("Clean"))),
            (SEARCH, node(Role::Search, Some("Search commands"))),
            (DRAWER, drawer),
            (UNNAMED, unnamed),
            (BUTTON, button),
        ],
        tree: Some(TreeInfo::new(WINDOW)),
        tree_id: TreeId::ROOT,
        focus: BUTTON,
    }
}

#[test]
#[ignore = "slice 6a: lands with patch 8 (PATCHES.md)"]
fn a_heading_reports_its_heading_level_and_no_level() {
    let requests = Requests::default();
    let context = context(&requests, tree());
    for (id, want) in [(TITLE, HeadingLevel1), (SECTION, HeadingLevel2)] {
        let heading = element(&context, id);
        assert_eq!(
            int(&property(&heading, UIA_HeadingLevelPropertyId)),
            Some(want.0),
            "{id:?}"
        );
        assert_eq!(int(&property(&heading, UIA_LevelPropertyId)), None, "{id:?}");
    }
    let item = element(&context, ITEM);
    assert_eq!(int(&property(&item, UIA_LevelPropertyId)), Some(3));
    let none = int(&property(&item, UIA_HeadingLevelPropertyId));
    assert!(
        none.is_none() || none == Some(HeadingLevel_None.0),
        "{none:?}"
    );
}

#[test]
#[ignore = "slice 6a: lands with patch 9 (PATCHES.md)"]
fn landmarks_report_their_type() {
    let requests = Requests::default();
    let context = context(&requests, tree());
    for (id, want) in [
        (MAIN, UIA_MainLandmarkTypeId),
        (RAIL, UIA_NavigationLandmarkTypeId),
        (SEARCH, UIA_SearchLandmarkTypeId),
    ] {
        let landmark = element(&context, id);
        assert_eq!(
            int(&property(&landmark, UIA_LandmarkTypePropertyId)),
            Some(want.0),
            "{id:?}"
        );
    }
}

#[test]
#[ignore = "slice 6a: lands with patch 9 (PATCHES.md)"]
fn a_named_region_is_a_custom_landmark_with_its_kind() {
    let requests = Requests::default();
    let context = context(&requests, tree());
    let drawer = element(&context, DRAWER);
    assert_eq!(
        int(&property(&drawer, UIA_LandmarkTypePropertyId)),
        Some(UIA_CustomLandmarkTypeId.0)
    );
    assert_eq!(
        text(&property(&drawer, UIA_LocalizedLandmarkTypePropertyId)).as_deref(),
        Some("region")
    );
    // Its control type's word stays UI Automation's default ("group"), not the kind.
    assert_eq!(
        text(&property(&drawer, UIA_LocalizedControlTypePropertyId)),
        None
    );
    let unnamed = element(&context, UNNAMED);
    let none = int(&property(&unnamed, UIA_LandmarkTypePropertyId));
    assert!(none.is_none() || none == Some(0), "{none:?}");
}

#[test]
#[ignore = "slice 6a: lands with patch 10 (PATCHES.md)"]
fn a_keyboard_shortcut_is_the_accelerator_key() {
    let requests = Requests::default();
    let context = context(&requests, tree());
    let button = element(&context, BUTTON);
    assert_eq!(
        text(&property(&button, UIA_AcceleratorKeyPropertyId)).as_deref(),
        Some("Ctrl+Shift+R")
    );
}
