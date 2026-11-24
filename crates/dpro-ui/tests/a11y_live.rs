//! Live regions (TD-0009 A3.5 addition 5, A4.4, A4.9), slice 6a's part: a label's or a named
//! container's politeness reaches a client, `Live::Off` sets nothing, other roles ignore it, and a
//! live node's text change is in the next update. Notices (A4.5) are slice 7b's and join this file
//! with it.

#![allow(missing_docs)]

use dpro_ui::{
    Element, FakeText, Key, Live, Motion, Size, Ui,
    accesskit::{Live as AkLive, Node, TreeUpdate},
    tokens,
};

const STATUS: Key = Key::name("status");
const BOX: Key = Key::name("box");
const BUTTON: Key = Key::name("button");

fn ui() -> Ui<u8> {
    Ui::new(
        Size { w: 400.0, h: 300.0 },
        1.0,
        Motion {
            hover_ms: tokens::motion::HOVER_MS,
        },
    )
}

/// A status line live as `live`, saying `text`; a named container live as `live`; and a button
/// asked to be live, which it can't be.
fn build(ui: &mut Ui<u8>, text: &str, live: Live) {
    ui.build(|cx| {
        cx.add(Element::text(STATUS, text).live(live));
        cx.add_with(Element::stack(BOX).name("Results").live(live), |cx| {
            cx.add(Element::text(Key::name("inner"), "inside"));
        });
        cx.add(Element::button(BUTTON, "Run", 1).live(live));
    });
    ui.layout(&mut FakeText);
}

/// Node `key` (a root child) as `tree` has it.
fn node<'t>(ui: &Ui<u8>, tree: &'t TreeUpdate, key: Key) -> Option<&'t Node> {
    let id = ui.find(&[key])?;
    tree.nodes
        .iter()
        .find(|(ak, _)| ui.a11y_node(*ak) == Some(id))
        .map(|(_, n)| n)
}

#[test]
fn live_off_sets_nothing() {
    let mut ui = ui();
    build(&mut ui, "Ready", Live::Off);
    let tree = ui.a11y_tree();
    assert!(tree.nodes.iter().all(|(_, n)| n.live().is_none()));
}

#[test]
fn a_label_and_a_named_container_carry_their_politeness_and_a_button_ignores_it() {
    for (live, want) in [
        (Live::Polite, AkLive::Polite),
        (Live::Assertive, AkLive::Assertive),
    ] {
        let mut ui = ui();
        build(&mut ui, "Scanning", live);
        let tree = ui.a11y_tree();
        assert_eq!(node(&ui, &tree, STATUS).and_then(Node::live), Some(want));
        assert_eq!(node(&ui, &tree, BOX).and_then(Node::live), Some(want));
        assert_eq!(node(&ui, &tree, BUTTON).and_then(Node::live), None);
    }
}

#[test]
fn a_live_nodes_text_change_is_in_the_next_update_and_its_politeness_change_too() {
    let mut ui = ui();
    build(&mut ui, "Scanning", Live::Polite);
    drop(ui.a11y_tree());
    build(&mut ui, "Scanned 7 categories", Live::Polite);
    let update = ui.a11y_update().expect("an update");
    let status = node(&ui, &update, STATUS).expect("the status line is sent");
    assert_eq!(
        status.value().or(status.label()),
        Some("Scanned 7 categories")
    );
    assert_eq!(status.live(), Some(AkLive::Polite));
    build(&mut ui, "Scanned 7 categories", Live::Assertive);
    let update = ui.a11y_update().expect("a politeness change is sent");
    assert_eq!(
        node(&ui, &update, STATUS).and_then(Node::live),
        Some(AkLive::Assertive)
    );
    build(&mut ui, "Scanned 7 categories", Live::Assertive);
    assert!(ui.a11y_update().is_none(), "nothing changed, nothing sent");
}

// ---- notices (TD-0009 A4.5, A4.9; slice 7b) ----------------------------------------------------

use dpro_app::announce::Announcement;
use dpro_core::{id::JobId, units::UnixMillis};
use dpro_ui::{Activity, Notice, NoticeKind};

/// A notice of `activity` saying `text`, replacing its activity's older ones.
fn notice(text: &str, activity: Activity) -> Notice<'_> {
    Notice {
        text,
        kind: NoticeKind::Other,
        important: false,
        replace: true,
        activity,
    }
}

/// The queue, drained: each notice's text and activity, oldest first.
fn drain(ui: &mut Ui<u8>) -> Vec<(String, Activity)> {
    let mut out = Vec::new();
    ui.take_notices(|n| out.push((n.text.to_owned(), n.activity)));
    out
}

/// An empty queue, drained.
const fn nothing() -> Vec<(String, Activity)> {
    Vec::new()
}

/// A window with a client listening.
fn listened() -> Ui<u8> {
    let mut ui = ui();
    build(&mut ui, "Ready", Live::Off);
    drop(ui.a11y_tree());
    ui
}

#[test]
fn without_a_client_a_notice_is_dropped_at_once() {
    let mut ui = ui();
    build(&mut ui, "Ready", Live::Off);
    ui.notify(notice("Copied.", Activity::Copy));
    assert_eq!(drain(&mut ui), nothing());
    // Nor is one kept for a client that comes later.
    drop(ui.a11y_tree());
    assert_eq!(drain(&mut ui), nothing());
    // A client that leaves takes the queue with it.
    ui.notify(notice("Copied.", Activity::Copy));
    ui.a11y_deactivate();
    assert_eq!(drain(&mut ui), nothing());
}

#[test]
fn notices_are_queued_in_order_replaced_by_activity_and_drained() {
    let mut ui = listened();
    ui.notify(notice("Apply: 10 percent", Activity::Progress(0)));
    ui.notify(notice("Remove: 10 percent", Activity::Progress(1)));
    ui.notify(Notice {
        replace: false,
        ..notice("Copied.", Activity::Copy)
    });
    ui.notify(Notice {
        replace: false,
        ..notice("Copied.", Activity::Copy)
    });
    // A newer step of job 0 drops its older one, wherever it waits.
    ui.notify(notice("Apply: 20 percent", Activity::Progress(0)));
    assert_eq!(
        drain(&mut ui),
        [
            ("Remove: 10 percent".to_owned(), Activity::Progress(1)),
            ("Copied.".to_owned(), Activity::Copy),
            ("Copied.".to_owned(), Activity::Copy),
            ("Apply: 20 percent".to_owned(), Activity::Progress(0)),
        ]
    );
    assert_eq!(drain(&mut ui), nothing(), "drained once");
    // A replacing notice drops every queued one of its activity, replacing or not.
    for _ in 0..2 {
        ui.notify(Notice {
            replace: false,
            ..notice("Copied.", Activity::Copy)
        });
    }
    ui.notify(notice("Copied again.", Activity::Copy));
    assert_eq!(
        drain(&mut ui),
        [("Copied again.".to_owned(), Activity::Copy)]
    );
}

#[test]
fn the_queue_holds_eight_and_drops_the_oldest() {
    let mut ui = listened();
    let texts: Vec<String> = (0..10).map(|n| format!("{n} items listed")).collect();
    for t in &texts {
        ui.notify(Notice {
            replace: false,
            ..notice(t, Activity::Count)
        });
    }
    let got: Vec<String> = drain(&mut ui).into_iter().map(|(t, _)| t).collect();
    assert_eq!(got, texts[2..]);
}

#[test]
fn text_is_sanitized_as_a_field_takes_it_and_capped_at_256_characters() {
    let mut ui = listened();
    // Line breaks dropped, a tab as a space, control characters and bidi overrides dropped.
    ui.notify(notice(
        "Remove\r\n\u{202E}evil\u{202C}\tpack\u{7}age\u{2066}: 40 percent",
        Activity::Progress(0),
    ));
    let long = "é".repeat(300);
    ui.notify(notice(&long, Activity::Progress(1)));
    let got = drain(&mut ui);
    assert_eq!(got[0].0, "Removeevil package: 40 percent");
    assert_eq!(got[1].0.chars().count(), 256);
    assert!(got[1].0.chars().all(|c| c == 'é'));
    // Nothing is left of a notice whose text is all control characters.
    ui.notify(notice("\u{1}\u{2}\r\n", Activity::Copy));
    assert_eq!(drain(&mut ui), nothing());
}

#[test]
fn announcements_are_delivered_as_a4_4_s_table_says() {
    let job = JobId::from_parts(UnixMillis(1_800_000_000_000), 1);
    let cases = [
        (
            Announcement::Progress {
                job,
                slot: 3,
                percent: 40,
            },
            NoticeKind::Other,
            false,
            Activity::Progress(3),
        ),
        (
            Announcement::Copied,
            NoticeKind::ActionCompleted,
            false,
            Activity::Copy,
        ),
        (
            Announcement::Results(3),
            NoticeKind::Other,
            false,
            Activity::Count,
        ),
        (
            Announcement::FilterCount {
                shown: 4,
                total: 100,
            },
            NoticeKind::Other,
            false,
            Activity::Count,
        ),
        (
            Announcement::Listed(12),
            NoticeKind::ItemAdded,
            false,
            Activity::Count,
        ),
        (
            Announcement::HoldReleased,
            NoticeKind::ActionAborted,
            true,
            Activity::Hold,
        ),
    ];
    for (a, kind, important, activity) in cases {
        let n = Notice::of(&a, "text");
        assert_eq!(
            (n.text, n.kind, n.important, n.replace, n.activity),
            ("text", kind, important, true, activity),
            "{a:?}"
        );
    }
}
