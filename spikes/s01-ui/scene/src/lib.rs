//! The shared model of spike S-01 (roadmap 1.6, ADR-0002).
//!
//! Both prototypes draw the same three screens from this crate, at the same coordinates, so one
//! set of pixel checks ([`check`]) and one scroll script ([`script`]) judges both. No OS
//! dependencies: the headless gates run in a Linux container (C1).

pub mod check;
pub mod data;
pub mod layout;
pub mod png;
pub mod script;
pub mod stats;
pub mod tokens;

/// The three screens of ADR-0002's spike: target switcher + nav, the 100k-row list, the preview drawer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    /// Title bar with the target pill and search, the grouped nav rail, the Overview page.
    Shell,
    /// The Apps page: a virtualized table over [`data::ROWS`] rows.
    List,
    /// The Apps page with the preview drawer open over it.
    Drawer,
}

impl Screen {
    pub const ALL: [Self; 3] = [Self::Shell, Self::List, Self::Drawer];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::List => "list",
            Self::Drawer => "drawer",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.name() == s)
    }
}

/// Everything a frame depends on. Both prototypes render a frame from this alone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct State {
    pub screen: Screen,
    pub theme: tokens::Theme,
    /// Scroll offset of the list, in logical pixels.
    pub scroll: f32,
    /// Index into [`data::NAV`] of the selected item.
    pub nav: usize,
    /// Selected row of the list.
    pub selected: Option<usize>,
}

impl State {
    #[must_use]
    pub const fn new(screen: Screen, theme: tokens::Theme) -> Self {
        Self {
            screen,
            theme,
            scroll: 0.0,
            nav: match screen {
                Screen::Shell => data::NAV_OVERVIEW,
                Screen::List | Screen::Drawer => data::NAV_APPS,
            },
            selected: match screen {
                Screen::Shell | Screen::List => None,
                Screen::Drawer => Some(data::SNAPSHOT_SELECTED),
            },
        }
    }

    /// The state the pixel snapshots are taken in: the list scrolled to a fixed, unaligned offset.
    #[must_use]
    pub const fn snapshot(screen: Screen, theme: tokens::Theme) -> Self {
        let mut s = Self::new(screen, theme);
        if !matches!(screen, Screen::Shell) {
            s.scroll = data::SNAPSHOT_SCROLL;
        }
        s
    }
}
