//! TD-0009 §7 U10: rank 1,000 palette entries (commands, targets, settings pages) per keystroke.
//!
//! Budget ≤ 1 ms: a share of U2's 2 ms per-event UI-thread budget (doc 04 §5).
//! C1: `docker compose run --rm bench -p dpro-app --bench palette`.

#![allow(missing_docs)]

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use dpro_app::palette::{PaletteEntry, Score, rank};

/// 1,000 labels shaped like the real ones: an action and its object ("Remove package"), with
/// localized keywords and 20 recently used entries.
fn entries() -> Vec<PaletteEntry> {
    const VERBS: [&str; 20] = [
        "Remove",
        "Add",
        "Enable",
        "Disable",
        "Open",
        "Export",
        "Import",
        "Scan",
        "Repair",
        "Clean",
        "Show",
        "Hide",
        "Undo",
        "Mount",
        "Unmount",
        "Apply",
        "Capture",
        "Uninstall",
        "Restore",
        "Back up",
    ];
    const NOUNS: [&str; 50] = [
        "package",
        "feature",
        "driver",
        "capability",
        "update",
        "app",
        "startup item",
        "service",
        "scheduled task",
        "image",
        "WIM index",
        "ESD file",
        "ISO",
        "VHDX",
        "boot entry",
        "recovery environment",
        "component store",
        "temp files",
        "logs",
        "Windows Update cache",
        "Delivery Optimization files",
        "thumbnail cache",
        "error reports",
        "event logs",
        "hard links",
        "registry leftovers",
        "Store app",
        "provisioned app",
        "language pack",
        "font",
        "hosts file",
        "file association",
        "context menu",
        "telemetry",
        "Copilot",
        "Recall",
        "Defender exclusions",
        "firewall rule",
        "power plan",
        "page file",
        "hibernation",
        "restore point",
        "shadow copy",
        "journal entry",
        "recipe",
        "unattend file",
        "answer file",
        "driver backup",
        "target",
        "settings",
    ];
    let mut v = Vec::with_capacity(VERBS.len() * NOUNS.len());
    for (i, verb) in VERBS.iter().enumerate() {
        for (j, noun) in NOUNS.iter().enumerate() {
            let label = format!("{verb} {noun}");
            let keywords = format!("{noun} {verb} dism servicing");
            // Every 50th entry was used recently (ranks 0..20).
            let recent = if j == 0 { u8::try_from(i).ok() } else { None };
            v.push(PaletteEntry::new(&label, &keywords, recent));
        }
    }
    v
}

fn palette(c: &mut Criterion) {
    let entries = entries();
    let mut out: Vec<(u32, Score)> = Vec::with_capacity(entries.len());
    let mut g = c.benchmark_group("palette_rank_1000");
    // Typing "remove p" one key at a time, an acronym, a keyword-only hit, and no match.
    for q in [
        "",
        "r",
        "re",
        "rem",
        "remove",
        "remove p",
        "rp",
        "servicing",
        "zqx",
    ] {
        g.bench_with_input(BenchmarkId::from_parameter(format!("{q:?}")), q, |b, q| {
            b.iter(|| rank(black_box(q), &entries, &mut out));
        });
    }
    g.finish();
}

criterion_group!(benches, palette);
criterion_main!(benches);
