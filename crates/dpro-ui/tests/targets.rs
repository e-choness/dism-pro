//! TD-0009 §4.4 and §10: every target is at least 24 × 24 effective pixels (doc 05 §6, WCAG 2.2's
//! minimum target size), on every view of the shell fixture: the table page, the drawer open over
//! it, a summary page and Settings.
//!
//! A target is any enabled node that takes input: buttons (table rows and rail pages included),
//! check boxes, fields, tabs, radio buttons and dropdowns (a popup's items are checked in
//! `popups.rs`). Their size is the layout's, in logical pixels, which are effective pixels at any
//! scale.

#[path = "../benches/fixtures/shell.rs"]
mod shell;

use dpro_ui::{NodeId, Role, Ui};
use shell::{Msg, Run, SETTINGS, Shell, Step};

/// The smallest target, in effective pixels.
const MIN: f32 = 24.0;

/// Every enabled target under `id` smaller than [`MIN`] either way, described.
fn too_small(ui: &Ui<Msg>, id: NodeId, out: &mut Vec<String>) {
    for c in ui.children(id) {
        let Some(n) = ui.info(c) else { continue };
        let target = matches!(
            n.role,
            Role::Button | Role::CheckBox | Role::Field | Role::Tab | Role::Radio | Role::ComboBox
        );
        if target && n.enabled && (n.rect.w < MIN || n.rect.h < MIN) {
            out.push(format!(
                "{:?} {:?} is {} × {}",
                n.role, n.text, n.rect.w, n.rect.h
            ));
        }
        too_small(ui, c, out);
    }
}

#[test]
fn every_target_is_at_least_24_px_on_every_view() {
    let mut run = Run::with(Shell::new());
    let views = [
        ("the table page", None),
        ("the drawer open", Some(Msg::Drawer)),
        ("a summary page", Some(Msg::Page(1))),
        ("Settings", Some(Msg::Page(SETTINGS))),
    ];
    let mut small = Vec::new();
    let mut targets = 0;
    for (name, msg) in views {
        if let Some(m) = msg {
            run.step(Step::Update(m));
        }
        let mut here = Vec::new();
        too_small(&run.ui, run.ui.root(), &mut here);
        small.extend(here.into_iter().map(|s| format!("{name}: {s}")));
        targets += count(&run.ui, run.ui.root());
    }
    assert!(targets > 0, "no targets at all");
    assert!(
        small.is_empty(),
        "targets under {MIN} px:\n{}",
        small.join("\n")
    );
}

/// Enabled targets under `id`.
fn count(ui: &Ui<Msg>, id: NodeId) -> usize {
    ui.children(id)
        .map(|c| {
            let own = ui.info(c).is_some_and(|n| {
                n.enabled
                    && matches!(
                        n.role,
                        Role::Button
                            | Role::CheckBox
                            | Role::Field
                            | Role::Tab
                            | Role::Radio
                            | Role::ComboBox
                    )
            });
            usize::from(own) + count(ui, c)
        })
        .sum()
}
