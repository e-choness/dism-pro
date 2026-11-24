//! The announcer's rate rules (TD-0009 A4.4, A4.5, A4.7 U22; slice 7b): progress is spoken only
//! on a new 10 % step, at most once per 2 s per job and once per second overall, never for an
//! unknown total, at 0 or at 100 %, and a lower step is a new phase. Eight jobs at once; a ninth
//! is silent. A proptest checks the rules over monotonic event sequences.

#![allow(missing_docs)]

use dpro_app::announce::{ANY_GAP_MS, Announcement, Announcer, JOB_GAP_MS, SLOTS};
use dpro_core::{id::JobId, progress::Progress, units::UnixMillis};
use proptest::prelude::*;

fn job(n: u64) -> JobId {
    JobId::from_parts(UnixMillis(1_800_000_000_000), u128::from(n))
}

/// Job `j`'s place in the generated run's table.
fn ix(j: u64) -> usize {
    usize::try_from(j).unwrap_or(0)
}

fn at(permille: u64) -> Progress {
    Progress {
        done: permille,
        total: 1000,
    }
}

/// The percentage `said` speaks, if it's `job`'s progress.
fn percent(said: Option<Announcement>, of: JobId) -> Option<u8> {
    match said? {
        Announcement::Progress { job, percent, .. } if job == of => Some(percent),
        _ => None,
    }
}

#[test]
fn ten_percent_steps_two_seconds_per_job_and_one_second_overall() {
    let (a, b) = (job(1), job(2));
    let mut an = Announcer::new();
    an.started(a, 0);
    an.started(b, 0);
    // Nothing within 2 s of the start, even past a step.
    assert_eq!(percent(an.progress(a, at(150), 1_999), a), None);
    // The next event after it says the current step.
    assert_eq!(percent(an.progress(a, at(160), 2_000), a), Some(10));
    // The same step again: nothing.
    assert_eq!(percent(an.progress(a, at(190), 6_000), a), None);
    // Another job, 500 ms after any notice: nothing; at 1 s: spoken.
    assert_eq!(percent(an.progress(b, at(420), 2_500), b), None);
    assert_eq!(percent(an.progress(b, at(430), 3_000), b), Some(40));
    // The first job's next step, 1.5 s after its notice: nothing; at 2 s it is, but 1 s after b's.
    assert_eq!(percent(an.progress(a, at(250), 3_500), a), None);
    assert_eq!(percent(an.progress(a, at(260), 4_000), a), Some(20));
}

#[test]
fn a_step_skipped_by_the_gap_is_said_with_the_next_event_as_the_current_step() {
    let a = job(1);
    let mut an = Announcer::new();
    an.started(a, 0);
    assert_eq!(percent(an.progress(a, at(100), 2_000), a), Some(10));
    // 20 % and 30 % pass within the gap: neither is said, and no timer waits for them.
    assert_eq!(percent(an.progress(a, at(200), 2_500), a), None);
    assert_eq!(percent(an.progress(a, at(370), 3_900), a), None);
    // The next event past the gap says where the job is now.
    assert_eq!(percent(an.progress(a, at(380), 4_000), a), Some(30));
}

#[test]
fn silence_for_an_unknown_total_at_zero_and_at_one_hundred() {
    let a = job(1);
    let mut an = Announcer::new();
    an.started(a, 0);
    let unknown = Progress {
        done: 500,
        total: 0,
    };
    assert_eq!(an.progress(a, unknown, 5_000), None);
    assert_eq!(an.progress(a, at(50), 6_000), None);
    assert_eq!(an.progress(a, at(1000), 9_000), None);
    // Completion isn't a step either: past 100 % is clamped to it.
    assert_eq!(
        an.progress(
            a,
            Progress {
                done: 2000,
                total: 1000
            },
            12_000
        ),
        None
    );
}

#[test]
fn a_lower_step_is_a_new_phase_recorded_and_not_said() {
    let a = job(1);
    let mut an = Announcer::new();
    an.started(a, 0);
    assert_eq!(percent(an.progress(a, at(900), 3_000), a), Some(90));
    // A second phase starts over: nothing is said for it going down.
    assert_eq!(percent(an.progress(a, at(0), 6_000), a), None);
    assert_eq!(percent(an.progress(a, at(300), 9_000), a), Some(30));
    // Down within the phase to a lower step that isn't 0: recorded, then each new step is said.
    assert_eq!(percent(an.progress(a, at(200), 12_000), a), None);
    assert_eq!(percent(an.progress(a, at(300), 15_000), a), Some(30));
}

#[test]
fn eight_slots_a_ninth_job_is_silent_and_a_finished_one_frees_its_slot() {
    let mut an = Announcer::new();
    for n in 0..9 {
        an.started(job(n), 0);
    }
    let mut slots = Vec::new();
    for n in 0..9 {
        match an.progress(job(n), at(500), 10_000 + 1_000 * n) {
            Some(Announcement::Progress { slot, percent, .. }) => {
                assert_eq!(percent, 50);
                slots.push(slot);
            }
            other => assert!(n == 8 && other.is_none(), "job {n}: {other:?}"),
        }
    }
    slots.sort_unstable();
    assert_eq!(slots, (0..8).collect::<Vec<u8>>(), "eight distinct slots");
    assert_eq!(SLOTS, 8);
    // A job never started is silent; once one finishes, a new one takes its slot.
    assert_eq!(an.progress(job(20), at(500), 30_000), None);
    an.finished(job(3));
    an.started(job(20), 30_000);
    assert!(matches!(
        an.progress(job(20), at(500), 40_000),
        Some(Announcement::Progress { slot: 3, .. })
    ));
    // The finished job is silent from now on.
    assert_eq!(an.progress(job(3), at(900), 50_000), None);
}

#[test]
fn every_announcement_names_a_message_of_the_catalog() {
    let catalog = include_str!("../../../i18n/en-US/ui.ftl");
    let all = [
        Announcement::Progress {
            job: job(1),
            slot: 0,
            percent: 40,
        },
        Announcement::Copied,
        Announcement::Results(3),
        Announcement::FilterCount {
            shown: 4,
            total: 100,
        },
        Announcement::Listed(12),
        Announcement::HoldReleased,
    ];
    for a in all {
        let id = a.id();
        assert!(id.starts_with("a11y-"), "{id}");
        assert!(
            catalog.lines().any(|l| l.starts_with(&format!("{id} ="))),
            "{id} isn't in i18n/en-US/ui.ftl"
        );
    }
}

/// One event of a generated run: a job (of 10) starts, ends, or reports progress (`None` is an
/// unknown total), after `dt` ms.
#[derive(Clone, Debug)]
enum Event {
    Start(u64),
    Finish(u64),
    Progress(u64, Option<u64>),
}

fn event() -> impl Strategy<Value = (u64, Event)> {
    let e = prop_oneof![
        1 => (0..10_u64).prop_map(Event::Start),
        1 => (0..10_u64).prop_map(Event::Finish),
        8 => (0..10_u64, proptest::option::weighted(0.9, 0..=1100_u64))
            .prop_map(|(j, p)| Event::Progress(j, p)),
    ];
    (0..1_500_u64, e)
}

/// What the rules allow for one job, tracked beside the announcer.
#[derive(Clone, Copy, Default)]
struct Seen {
    running: bool,
    start: u64,
    last_said: Option<u64>,
    /// The step last spoken, or the lower one a new phase came back to.
    floor: u8,
}

proptest! {
    #[test]
    fn the_rate_rules_hold_over_any_monotonic_run(events in proptest::collection::vec(event(), 1..300)) {
        let mut an = Announcer::new();
        let mut seen = [Seen::default(); 10];
        let mut now = 0_u64;
        let mut last_any: Option<u64> = None;
        for (dt, e) in events {
            now += dt;
            match e {
                Event::Start(j) => {
                    an.started(job(j), now);
                    let running = seen.iter().filter(|s| s.running).count();
                    if seen[ix(j)].running || running < SLOTS {
                        seen[ix(j)] = Seen { running: true, start: now, last_said: None, floor: 0 };
                    }
                }
                Event::Finish(j) => {
                    an.finished(job(j));
                    seen[ix(j)].running = false;
                }
                Event::Progress(j, permille) => {
                    let p = Progress { done: permille.unwrap_or(0), total: if permille.is_some() { 1000 } else { 0 } };
                    let said = an.progress(job(j), p, now);
                    let s = &mut seen[ix(j)];
                    let step = p.permille().map(|m| u8::try_from(m / 100).unwrap_or(10));
                    if let Some(step) = step && step < s.floor {
                        s.floor = step;
                    }
                    let Some(said) = said else { continue };
                    let Announcement::Progress { job: who, percent, .. } = said else {
                        prop_assert!(false, "only progress is announced: {said:?}");
                        continue;
                    };
                    prop_assert_eq!(who, job(j));
                    prop_assert!(s.running, "job {} isn't running", j);
                    let step = step.unwrap_or_default();
                    prop_assert!((1..=9).contains(&step), "step {}", step);
                    prop_assert_eq!(u32::from(percent), u32::from(step) * 10);
                    prop_assert!(step > s.floor, "step {} isn't past {}", step, s.floor);
                    prop_assert!(now - s.last_said.unwrap_or(s.start) >= JOB_GAP_MS);
                    if let Some(t) = last_any {
                        prop_assert!(now - t >= ANY_GAP_MS);
                    }
                    s.last_said = Some(now);
                    s.floor = step;
                    last_any = Some(now);
                }
            }
        }
    }
}
