//! What the three screens show. The strings are spike-only: the product takes every string from
//! Fluent IDs (the no-string-literals UI rule, ADR-0005).

/// Rows in the virtualized list (ADR-0002: "100k-row virtualized list").
pub const ROWS: usize = 100_000;

/// The list offset the pixel snapshots use: deep in the list and not row-aligned.
pub const SNAPSHOT_SCROLL: f32 = 1_234_567.0;
/// The row selected in the drawer snapshot (visible at [`SNAPSHOT_SCROLL`]).
pub const SNAPSHOT_SELECTED: usize = 38_585;

pub const APP_NAME: &str = "DISM Pro";
pub const TARGET: &str = "This PC \u{b7} Windows Server 2025 \u{b7} C:";
pub const SEARCH: &str = "Search or run a command  (Ctrl+K)";
pub const ACTIVITY: &str = "Activity";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Item(&'static str),
    Group(&'static str),
    Separator,
}

/// doc 05 §3 information architecture.
pub const NAV: [Nav; 18] = [
    Nav::Item("Overview"),
    Nav::Group("Maintain"),
    Nav::Item("Clean"),
    Nav::Item("Repair"),
    Nav::Item("Updates"),
    Nav::Group("Customize"),
    Nav::Item("Apps"),
    Nav::Item("Optimize"),
    Nav::Item("Features"),
    Nav::Item("Drivers"),
    Nav::Group("Deploy"),
    Nav::Item("Images"),
    Nav::Item("Unattend"),
    Nav::Item("Boot & Recovery"),
    Nav::Separator,
    Nav::Item("Recipes"),
    Nav::Item("Activity"),
    Nav::Item("Settings"),
];
pub const NAV_OVERVIEW: usize = 0;
pub const NAV_APPS: usize = 6;

pub const OVERVIEW_TITLE: &str = "Overview";
pub const CARDS: [(&str, &str); 4] = [
    ("Health", "No issues"),
    ("Reclaimable", "4.21 GB"),
    ("Pending reboot", "None"),
    ("Last scan", "Today 09:12"),
];
pub const RECENT_TITLE: &str = "Recent activity";
pub const RECENT: [(&str, Reversibility); 5] = [
    (
        "Cleaned Windows Update cache (2.31 GB)",
        Reversibility::Permanent,
    ),
    ("Disabled 3 startup items", Reversibility::Undoable),
    ("Removed Contoso Toolbox 4.2", Reversibility::UndoableDays),
    ("Enabled TelnetClient", Reversibility::Undoable),
    ("Repaired component store", Reversibility::Permanent),
];

pub const APPS_TITLE: &str = "Apps";
pub const COLUMNS: [&str; 5] = ["Name", "Publisher", "Version", "Size", "Installed"];

pub const DRAWER_TITLE: &str = "Review changes";
pub const DRAWER_SUMMARY: &str = "12 changes \u{b7} 1.42 GB \u{b7} restart not needed";
pub const DRAWER_CLI: &str = "dismpro-cli apps uninstall --id contoso.toolbox --thorough --yes";
pub const DRAWER_CANCEL: &str = "Cancel";
pub const DRAWER_APPLY: &str = "Uninstall";
pub const CHANGES: [(&str, &str, Reversibility); 12] = [
    ("Run the uninstaller", "", Reversibility::Permanent),
    (
        "C:\\Program Files\\Contoso\\Toolbox",
        "812 MB",
        Reversibility::UndoableDays,
    ),
    (
        "C:\\ProgramData\\Contoso\\Toolbox",
        "301 MB",
        Reversibility::UndoableDays,
    ),
    (
        "%LOCALAPPDATA%\\Contoso\\Cache",
        "214 MB",
        Reversibility::UndoableDays,
    ),
    (
        "%APPDATA%\\Contoso\\Toolbox",
        "48.2 MB",
        Reversibility::UndoableDays,
    ),
    (
        "C:\\Windows\\Temp\\ctbx-setup",
        "39.9 MB",
        Reversibility::UndoableDays,
    ),
    (
        "HKLM\\SOFTWARE\\Contoso\\Toolbox",
        "34 values",
        Reversibility::Undoable,
    ),
    (
        "HKCU\\Software\\Contoso\\Toolbox",
        "12 values",
        Reversibility::Undoable,
    ),
    ("HKCR\\.ctbx", "3 values", Reversibility::Undoable),
    ("Service ContosoUpdate", "", Reversibility::Undoable),
    (
        "Scheduled task \\Contoso\\Update",
        "",
        Reversibility::Undoable,
    ),
    ("Start menu shortcut", "", Reversibility::Undoable),
];

/// doc 05 §5 reversibility badge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reversibility {
    Undoable,
    UndoableDays,
    Permanent,
}

impl Reversibility {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Undoable => "Undoable",
            Self::UndoableDays => "Undoable 7 days",
            Self::Permanent => "Permanent",
        }
    }
}

/// TD-0015 row badge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Badge {
    None,
    New,
    Changed,
    Orphan,
}

impl Badge {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "",
            Self::New => "New",
            Self::Changed => "Changed",
            Self::Orphan => "Orphan",
        }
    }
}

/// One text column in a single arena: no allocation per row, per frame (doc 04 §5).
#[derive(Debug, Default)]
pub struct TextColumn {
    arena: String,
    ends: Vec<u32>,
}

impl TextColumn {
    fn push(&mut self, s: &str) {
        self.arena.push_str(s);
        self.ends.push(self.arena.len() as u32);
    }

    #[must_use]
    pub fn get(&self, i: usize) -> &str {
        let start = if i == 0 { 0 } else { self.ends[i - 1] as usize };
        &self.arena[start..self.ends[i] as usize]
    }

    #[must_use]
    pub fn bytes(&self) -> usize {
        self.arena.len() + self.ends.len() * 4
    }
}

/// The list model, columnar (doc 04 §5: "List models are columnar for 100k-row views").
#[derive(Debug)]
pub struct Rows {
    pub cells: [TextColumn; 5],
    pub badge: Vec<Badge>,
}

const VENDORS: [&str; 24] = [
    "Contoso",
    "Fabrikam",
    "Northwind",
    "Tailspin",
    "Wingtip",
    "Adventure Works",
    "Litware",
    "Proseware",
    "Woodgrove",
    "Lamna",
    "Alpine Ski",
    "Coho",
    "Fourth Coffee",
    "Margie's Travel",
    "Trey Research",
    "Blue Yonder",
    "City Power",
    "Consolidated",
    "Graphic Design",
    "Humongous",
    "Lucerne",
    "Relecloud",
    "VanArsdel",
    "Wide World",
];
const PRODUCTS: [&str; 32] = [
    "Toolbox",
    "Studio",
    "Runtime",
    "Driver Pack",
    "Updater",
    "Media Player",
    "Backup",
    "Sync",
    "Designer",
    "Viewer",
    "Editor",
    "Analyzer",
    "Agent",
    "Connector",
    "SDK",
    "Redistributable",
    "Language Pack",
    "Control Center",
    "Monitor",
    "Launcher",
    "Toolkit",
    "Framework",
    "Helper",
    "Assistant",
    "Suite",
    "Manager",
    "Explorer",
    "Console",
    "Server",
    "Client",
    "Plugin",
    "Bridge",
];
const EDITIONS: [&str; 8] = [
    "",
    " Pro",
    " (x64)",
    " 2026",
    " Community",
    " LTS",
    " (ARM64)",
    " Plus",
];

impl Rows {
    /// The same rows on every run and platform (a fixed xorshift seed).
    #[must_use]
    pub fn generate(n: usize) -> Self {
        let mut rng = XorShift(0x9E37_79B9_7F4A_7C15);
        let mut cells: [TextColumn; 5] = Default::default();
        let mut badge = Vec::with_capacity(n);
        let mut s = String::new();
        for i in 0..n {
            let r = rng.next();
            let vendor = VENDORS[(r % 24) as usize];
            let product = PRODUCTS[((r >> 8) % 32) as usize];
            let edition = EDITIONS[((r >> 16) % 8) as usize];
            s.clear();
            push_fmt(
                &mut s,
                format_args!("{vendor} {product}{edition} {}", i % 997 + 1),
            );
            cells[0].push(&s);
            cells[1].push(vendor);
            s.clear();
            push_fmt(
                &mut s,
                format_args!("{}.{}.{}", (r >> 24) % 30, (r >> 32) % 20, (r >> 40) % 9000),
            );
            cells[2].push(&s);
            s.clear();
            write_size(&mut s, (r >> 20) % 8_000_000_000 + 40_000);
            cells[3].push(&s);
            s.clear();
            let day = (r >> 44) % 2400;
            push_fmt(
                &mut s,
                format_args!(
                    "20{:02}-{:02}-{:02}",
                    20 + day / 365,
                    (day % 365) / 31 + 1,
                    day % 28 + 1
                ),
            );
            cells[4].push(&s);
            badge.push(match (r >> 52) % 16 {
                0 => Badge::New,
                1 => Badge::Changed,
                2 => Badge::Orphan,
                _ => Badge::None,
            });
        }
        Self { cells, badge }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.badge.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.badge.is_empty()
    }

    #[must_use]
    pub fn cell(&self, row: usize, column: usize) -> &str {
        self.cells[column].get(row)
    }
}

fn push_fmt(s: &mut String, args: std::fmt::Arguments<'_>) {
    use std::fmt::Write as _;
    let _ = s.write_fmt(args); // writing to a String can't fail
}

fn write_size(s: &mut String, bytes: u64) {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    let mut v = bytes as f64 / 1000.0;
    let mut u = 0;
    while v >= 1000.0 && u < 3 {
        v /= 1000.0;
        u += 1;
    }
    let digits = if v >= 100.0 {
        0
    } else if v >= 10.0 {
        1
    } else {
        2
    };
    push_fmt(s, format_args!("{v:.digits$} {}", UNITS[u]));
}

struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_deterministic_and_columnar() {
        let a = Rows::generate(1000);
        let b = Rows::generate(1000);
        assert_eq!(a.len(), 1000);
        for i in [0, 1, 499, 999] {
            for c in 0..5 {
                assert_eq!(a.cell(i, c), b.cell(i, c));
                assert!(!a.cell(i, c).is_empty(), "row {i} column {c}");
            }
        }
        assert!(a.cell(0, 0).ends_with(" 1"), "{}", a.cell(0, 0));
    }

    #[test]
    fn sizes_read_like_the_cli() {
        let mut s = String::new();
        write_size(&mut s, 1_420_000_000);
        assert_eq!(s, "1.42 GB");
        s.clear();
        write_size(&mut s, 812_000_000);
        assert_eq!(s, "812 MB");
    }
}
