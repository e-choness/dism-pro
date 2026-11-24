//! Badges from the domain (TD-0009 A3.5 (1), A3.9): every `Reversibility` case, the day count
//! rounding up, an expired deadline, a plan's restart and the job states.

use dpro_app::badges::{self, Badge, Description, Text, Tone};
use dpro_core::{
    health::RebootReason,
    plan::{ChangeKind, PlanSummary, Reversibility, StartType},
    units::UnixMillis,
};

const DAY: u64 = 86_400_000;
const NOW: UnixMillis = UnixMillis(1_800_000_000_000);

fn until(ms: u64) -> Reversibility<&'static str> {
    Reversibility::ReversibleUntil {
        until: UnixMillis(NOW.0 + ms),
    }
}

#[test]
fn a_reversible_change_is_undoable_without_a_description() {
    let r = Reversibility::<&str>::Reversible;
    assert_eq!(
        badges::reversibility(&r, NOW),
        Badge {
            tone: Tone::Undoable,
            text: Text::Undoable,
            description: None,
        }
    );
}

#[test]
fn a_deadline_counts_whole_days_left_rounded_up() {
    for (left, days) in [(1, 1), (DAY - 1, 1), (DAY, 1), (DAY + 1, 2), (7 * DAY, 7)] {
        let r = until(left);
        let b = badges::reversibility(&r, NOW);
        assert_eq!(b.tone, Tone::Undoable, "{left} ms left");
        assert_eq!(b.text, Text::UndoableDays { days }, "{left} ms left");
        assert_eq!(
            b.description,
            Some(Description::UndoableUntil {
                until: UnixMillis(NOW.0 + left)
            }),
        );
        assert_eq!(b.text.id(), "ui-badge-undoable-days");
        assert_eq!(
            b.description.map(|d| d.id()),
            Some("ui-badge-undoable-until-desc")
        );
    }
}

#[test]
fn a_passed_deadline_is_permanent_and_says_expired() {
    for r in [
        Reversibility::<&str>::ReversibleUntil { until: NOW },
        Reversibility::ReversibleUntil {
            until: UnixMillis(NOW.0 - 1),
        },
        Reversibility::ReversibleUntil {
            until: UnixMillis(0),
        },
    ] {
        let b = badges::reversibility(&r, NOW);
        assert_eq!(
            b,
            Badge {
                tone: Tone::Permanent,
                text: Text::Expired,
                description: None,
            },
            "{r:?}"
        );
        assert_eq!(b.text.id(), "ui-badge-expired");
    }
}

#[test]
fn an_undo_by_an_inverse_change_is_undoable_and_names_the_inverse() {
    let inverse = ChangeKind::ServiceStart {
        name: "wuauserv",
        new: StartType::Demand,
    };
    let r = Reversibility::ReversibleVia {
        inverse: Box::new(inverse),
    };
    let b = badges::reversibility(&r, NOW);
    assert_eq!(b.tone, Tone::Undoable);
    assert_eq!(b.text, Text::Undoable);
    assert_eq!(
        b.description,
        Some(Description::UndoableVia { inverse: &inverse })
    );
    assert_eq!(
        b.description.map(|d| d.id()),
        Some("ui-badge-undoable-via-desc")
    );
}

#[test]
fn an_irreversible_change_is_permanent_and_gives_its_reason() {
    let r = Reversibility::Irreversible {
        reason: "plan-irreversible-delete",
    };
    let b = badges::reversibility(&r, NOW);
    assert_eq!(b.tone, Tone::Permanent);
    assert_eq!(b.text, Text::Permanent);
    assert_eq!(b.text.id(), "badge-permanent");
    assert_eq!(
        b.description,
        Some(Description::Permanent {
            reason: &"plan-irreversible-delete"
        })
    );
    assert_eq!(
        b.description.map(|d| d.id()),
        Some("ui-badge-permanent-desc")
    );
}

#[test]
fn a_restart_is_caution_described_by_its_reason_and_none_is_neutral() {
    let b = badges::restart::<&str>(true, Some(RebootReason::WindowsUpdate));
    assert_eq!(b.tone, Tone::Caution);
    assert_eq!(b.text, Text::Restart);
    assert_eq!(b.text.id(), "ui-badge-restart");
    assert_eq!(
        b.description.map(|d| d.id()),
        Some(RebootReason::WindowsUpdate.message_id())
    );

    let b = badges::restart::<&str>(true, None);
    assert_eq!(
        (b.tone, b.text, b.description),
        (Tone::Caution, Text::Restart, None)
    );

    let b = badges::restart::<&str>(false, Some(RebootReason::Cbs));
    assert_eq!(
        (b.tone, b.text, b.description),
        (Tone::Neutral, Text::NoRestart, None),
        "nothing to restart for: no reason to give"
    );
    assert_eq!(b.text.id(), "ui-badge-no-restart");
}

#[test]
fn a_plans_summary_gives_its_restart_badge() {
    let mut summary = PlanSummary {
        changes: 3,
        reboot_required: true,
        earliest_deadline: Some(UnixMillis(NOW.0 + DAY)),
        ..PlanSummary::default()
    };
    let b = badges::plan_restart::<&str>(&summary);
    assert_eq!((b.tone, b.text), (Tone::Caution, Text::Restart));
    summary.reboot_required = false;
    let b = badges::plan_restart::<&str>(&summary);
    assert_eq!((b.tone, b.text), (Tone::Neutral, Text::NoRestart));
}

#[test]
fn a_jobs_state_badges() {
    let running = badges::running::<&str>();
    assert_eq!(
        (running.tone, running.text.id()),
        (Tone::Info, "ui-badge-running")
    );
    let partial = badges::partial::<&str>();
    assert_eq!(
        (partial.tone, partial.text.id()),
        (Tone::Caution, "ui-badge-partial")
    );
    assert_eq!(running.description, None);
}

#[test]
fn every_text_has_its_own_fluent_id() {
    let all = [
        Text::Undoable,
        Text::UndoableDays { days: 3 },
        Text::Expired,
        Text::Permanent,
        Text::Restart,
        Text::NoRestart,
        Text::Running,
        Text::Partial,
    ];
    for (i, a) in all.iter().enumerate() {
        for b in &all[i + 1..] {
            assert_ne!(a.id(), b.id(), "{a:?} and {b:?}");
        }
    }
    assert_eq!(Text::Undoable.id(), "badge-undoable");
}
