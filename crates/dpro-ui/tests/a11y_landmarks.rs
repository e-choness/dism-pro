//! Headings and landmarks (TD-0009 A4.4, A4.5, A4.9), slice 7a: one level-1 heading per page and
//! level 2 for its sections; exactly one Main landmark, named by the page's title; the rail is a
//! Navigation landmark around its list, which keeps its role; the drawer a region with its kind;
//! landmarks in visual order. On the shell's summary page with the drawer open, and on the
//! composites page.
//!
//! AccessKit's level is zero-based (aria-level less one): a level-1 heading has `level()` 0, and
//! the adapter reports `HeadingLevel` 80050 + level + 1 (patch 8).

#![allow(missing_docs)]

#[path = "../benches/fixtures/composites.rs"]
mod composites;
#[path = "../benches/fixtures/shell.rs"]
mod shell;

use dpro_ui::{
    Element, FakeText, Key, Landmark, Motion, Rect, Size, Ui,
    accesskit::{Node, NodeId as AkId, Role, TreeUpdate},
    tokens,
};

/// The tree's nodes from its root, depth first: visual order for these pages.
fn order(tree: &TreeUpdate) -> Vec<(AkId, &Node)> {
    composites::Run::tree_order(tree)
}

fn headings(tree: &TreeUpdate) -> Vec<(usize, String)> {
    order(tree)
        .into_iter()
        .filter(|(_, n)| n.role() == Role::Heading)
        .map(|(_, n)| {
            (
                n.level().unwrap_or(usize::MAX),
                n.label().or(n.value()).unwrap_or_default().to_owned(),
            )
        })
        .collect()
}

fn landmarks(tree: &TreeUpdate) -> Vec<(Role, String, Option<String>)> {
    order(tree)
        .into_iter()
        .filter(|(_, n)| {
            matches!(
                n.role(),
                Role::Main | Role::Navigation | Role::Region | Role::Search
            )
        })
        .map(|(_, n)| {
            (
                n.role(),
                n.label().unwrap_or_default().to_owned(),
                n.role_description().map(str::to_owned),
            )
        })
        .collect()
}

/// The shell on a summary page with the drawer open, its whole tree.
fn shell_tree() -> TreeUpdate {
    let mut run = shell::Run::accessible(shell::Shell::new());
    run.step(shell::Step::Update(shell::Msg::Page(1)));
    run.step(shell::Step::Update(shell::Msg::Drawer));
    run.ui.a11y_tree()
}

#[test]
fn a_page_has_one_level_1_heading_its_title_and_sections_at_level_2() {
    let mut run = composites::Run::accessible(composites::Model::new());
    let found = headings(&run.ui.a11y_tree());
    assert_eq!(
        found.first(),
        Some(&(0, composites::TITLE.to_owned())),
        "{found:?}"
    );
    assert_eq!(
        found.iter().filter(|(l, _)| *l == 0).count(),
        1,
        "{found:?}"
    );
    assert!(
        found
            .iter()
            .any(|(l, t)| *l == 1 && t == composites::SETTINGS_HEADING),
        "{found:?}"
    );
    assert!(found.iter().all(|(l, _)| *l <= 1), "{found:?}");

    let shell = headings(&shell_tree());
    assert_eq!(
        shell.iter().filter(|(l, _)| *l == 0).count(),
        1,
        "{shell:?}"
    );
}

#[test]
fn exactly_one_main_landmark_named_by_the_page_title() {
    let mut run = composites::Run::accessible(composites::Model::new());
    let found = landmarks(&run.ui.a11y_tree());
    let mains: Vec<_> = found.iter().filter(|(r, _, _)| *r == Role::Main).collect();
    assert_eq!(mains.len(), 1, "{found:?}");
    assert_eq!(mains[0].1, composites::TITLE);
}

#[test]
fn the_shells_landmarks_are_in_visual_order_and_regions_have_a_kind() {
    let found = landmarks(&shell_tree());
    let roles: Vec<Role> = found.iter().map(|(r, _, _)| *r).collect();
    assert_eq!(
        roles,
        [Role::Navigation, Role::Main, Role::Region],
        "{found:?}"
    );
    assert_eq!(found[1].1, "Clean", "the page's title: {found:?}");
    assert_eq!(found[2].1, "Changes");
    assert_eq!(found[2].2.as_deref(), Some(shell::REGION));
    assert!(
        found.iter().all(|(_, name, _)| !name.is_empty()),
        "{found:?}"
    );
}

/// The rail is a Navigation landmark around its list, as a web `<nav>` holds a `<ul>` (A4.5):
/// the list keeps its role, so UI Automation sees a List of the 12 pages, "Overview" first, as
/// G1's U12 check finds it (run 45127764360 found none when the landmark replaced the list's
/// role), and Narrator reads a page's place in it (A4.9's N-2).
#[test]
fn the_rail_is_a_navigation_landmark_around_the_list_of_pages() {
    let mut run = shell::Run::accessible(shell::Shell::new());
    let tree = run.ui.a11y_tree();
    let ak = |id: AkId| {
        tree.nodes
            .iter()
            .find(|(ak, _)| *ak == id)
            .map(|(_, n)| n)
            .expect("a node of the tree")
    };
    let navs: Vec<_> = order(&tree)
        .into_iter()
        .filter(|(_, n)| n.role() == Role::Navigation)
        .collect();
    let [(nav, rail)] = navs.as_slice() else {
        panic!("one Navigation landmark: {navs:?}");
    };
    assert_eq!(rail.label(), Some("Pages"));
    let wrapper = run.ui.find(&shell::Run::RAIL_LIST[..2]);
    assert_eq!(
        run.ui.a11y_node(*nav),
        wrapper,
        "the container around the list"
    );
    let [list_id] = rail.children() else {
        panic!("the landmark holds the list alone: {:?}", rail.children());
    };
    let list = ak(*list_id);
    assert_eq!((list.role(), list.label()), (Role::List, Some("Pages")));
    assert_eq!(
        run.ui.a11y_node(*list_id),
        run.ui.find(&shell::Run::RAIL_LIST)
    );
    let pages: Vec<_> = list.children().iter().map(|&c| ak(c)).collect();
    assert_eq!(pages.len(), 12);
    assert!(pages.iter().all(|p| p.role() == Role::ListItem));
    assert_eq!(pages[0].label(), Some("Overview"));
    assert_eq!(pages[0].is_selected(), Some(true));

    // The landmark is the list's box, so the list is where it was without one: the body's left
    // 200 px, its whole height.
    let rect = |path: &[Key]| {
        let id = run.ui.find(path).expect("built");
        run.ui.info(id).expect("live").rect
    };
    let body = rect(&shell::Run::RAIL_LIST[..1]);
    let list = rect(&shell::Run::RAIL_LIST);
    assert_eq!(rect(&shell::Run::RAIL_LIST[..2]), list);
    assert_eq!(list, Rect { w: 200.0, ..body });
}

#[test]
fn heading_levels_are_clamped_to_1_through_9() {
    let mut ui: Ui<()> = Ui::new(
        Size { w: 400.0, h: 400.0 },
        1.0,
        Motion {
            hover_ms: tokens::motion::HOVER_MS,
        },
    );
    ui.build(|cx| {
        cx.add(Element::text(Key::name("zero"), "Zero").heading(0));
        cx.add(Element::text(Key::name("nine"), "Nine").heading(9));
        cx.add(Element::text(Key::name("big"), "Big").heading(200));
        cx.add_with(
            Element::stack(Key::name("search"))
                .name("Search")
                .landmark(Landmark::Search),
            |_| {},
        );
    });
    ui.layout(&mut FakeText);
    let tree = ui.a11y_tree();
    let levels: Vec<_> = headings(&tree).into_iter().map(|(l, _)| l).collect();
    assert_eq!(levels, [0, 8, 8]);
    assert_eq!(landmarks(&tree).first().map(|l| l.0), Some(Role::Search));
}
