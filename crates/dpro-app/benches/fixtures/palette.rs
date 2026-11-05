//! The U10 palette (TD-0009 §7): 1,000 entries and the queries typed into it. Shared by the
//! criterion bench (`palette.rs`) and the instruction-count bench (`ir.rs`).

use dpro_app::palette::PaletteEntry;

/// Typing "remove p" one key at a time, an acronym, a keyword-only hit, and no match.
pub(crate) const QUERIES: [&str; 9] = [
    "",
    "r",
    "re",
    "rem",
    "remove",
    "remove p",
    "rp",
    "servicing",
    "zqx",
];

/// 1,000 labels shaped like the real ones: an action and its object ("Remove package"), with
/// localized keywords and 20 recently used entries.
pub(crate) fn entries() -> Vec<PaletteEntry> {
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
