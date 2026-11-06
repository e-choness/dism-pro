//! The retained tree's pieces: keys, node ids, roles, the per-frame [`Element`] description, and
//! the long-lived node behind it (TD-0009 §5.1).

use dpro_app::table::Tri;

use crate::{
    field::TextEdit,
    geom::{Rect, Size},
    style::Style,
    text::TextStyle,
    theme::{Tone, tokens::radius},
};

/// A node's identity among its siblings. Reconciling matches nodes by key, so a key must be
/// unique among siblings and stable across frames: a name, or a data row's id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key(pub u64);

impl Key {
    /// A key from a name (FNV-1a of its bytes).
    #[must_use]
    #[allow(clippy::cast_lossless, reason = "u64::from isn't const")]
    pub const fn name(s: &str) -> Self {
        let b = s.as_bytes();
        let mut h = 0xcbf2_9ce4_8422_2325_u64;
        let mut i = 0;
        while i < b.len() {
            h ^= b[i] as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
            i += 1;
        }
        Self(h)
    }

    /// A key from a position or a data id, scrambled (splitmix64) so that ids and name hashes
    /// don't collide in practice.
    #[must_use]
    pub const fn index(i: u64) -> Self {
        let mut z = i.wrapping_add(0x9e37_79b9_7f4a_7c15);
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        Self(z ^ (z >> 31))
    }
}

/// A live node. Stable for the node's lifetime, so it can name the node to UI Automation; never
/// reused for another node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId {
    pub(crate) index: u32,
    pub(crate) generation: u32,
}

/// What a node is, for input and UI Automation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Lays out its children; not interactive.
    Container,
    Label,
    /// Emits its message when pressed. May hold children (a table row).
    Button,
    /// Emits its message when toggled; the view owns the state.
    CheckBox,
    /// One line of editable text (`field.rs`); the view owns the value.
    Field,
}

impl Role {
    pub(crate) fn interactive(self) -> bool {
        matches!(self, Self::Button | Self::CheckBox | Self::Field)
    }
}

/// How a node is painted, in theme tokens only (doc 05 §4). Changing it repaints the node; it never
/// changes layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look {
    /// The background, if any.
    pub fill: Option<Tone>,
    /// A one-pixel border inside the edge, if any.
    pub border: Option<Tone>,
    pub radius: Radius,
    /// The color of the node's text.
    pub ink: Tone,
}

impl Look {
    /// Nothing but primary text.
    pub const PLAIN: Self = Self {
        fill: None,
        border: None,
        radius: Radius::None,
        ink: Tone::Text,
    };
}

impl Default for Look {
    fn default() -> Self {
        Self::PLAIN
    }
}

/// A corner radius token.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Radius {
    #[default]
    None,
    /// Badges and check boxes.
    Small,
    Control,
    Card,
    /// Drawers and dialogs.
    Drawer,
}

impl Radius {
    /// In logical pixels.
    #[must_use]
    pub const fn logical(self) -> f32 {
        match self {
            Self::None => 0.0,
            Self::Small => radius::SMALL,
            Self::Control => radius::CONTROL,
            Self::Card => radius::CARD,
            Self::Drawer => radius::DRAWER,
        }
    }
}

/// One node as a view describes it for this frame. Cheap: the text is borrowed from the
/// view-model and copied only when it changed.
#[derive(Debug)]
pub struct Element<'s, M> {
    pub(crate) key: Key,
    pub(crate) role: Role,
    pub(crate) style: Style,
    pub(crate) text: &'s str,
    pub(crate) text_style: TextStyle,
    pub(crate) look: Look,
    pub(crate) msg: Option<M>,
    pub(crate) check: Tri,
    pub(crate) enabled: bool,
    pub(crate) selected: Option<bool>,
    /// A field's accessible name.
    pub(crate) label: &'s str,
    /// What a field shows while it's empty.
    pub(crate) placeholder: &'s str,
    /// A field's edits as the view's messages; `None` for a read-only field.
    pub(crate) on_edit: Option<fn(TextEdit) -> M>,
}

impl<'s, M> Element<'s, M> {
    fn new(key: Key, role: Role, text: &'s str, msg: Option<M>) -> Self {
        Self {
            key,
            role,
            style: Style::default(),
            text,
            text_style: TextStyle::default(),
            look: Look::PLAIN,
            msg,
            check: Tri::Off,
            enabled: true,
            selected: None,
            label: "",
            placeholder: "",
            on_edit: None,
        }
    }

    /// A container for children.
    #[must_use]
    pub fn stack(key: Key) -> Self {
        Self::new(key, Role::Container, "", None)
    }

    #[must_use]
    pub fn text(key: Key, text: &'s str) -> Self {
        Self::new(key, Role::Label, text, None)
    }

    /// Shows `label` when it has no children; with children (a row), `label` is its accessible
    /// name.
    #[must_use]
    pub fn button(key: Key, label: &'s str, msg: M) -> Self {
        Self {
            look: Look {
                radius: Radius::Control,
                ..Look::PLAIN
            },
            ..Self::new(key, Role::Button, label, Some(msg))
        }
    }

    #[must_use]
    pub fn toggle(key: Key, label: &'s str, state: Tri, msg: M) -> Self {
        Self {
            check: state,
            ..Self::new(key, Role::CheckBox, label, Some(msg))
        }
    }

    /// A one-line text field showing `value`. Each edit reaches the view as `on_edit(edit)`; the
    /// view applies it to its copy ([`TextEdit::apply`]) and passes the result as `value` next
    /// frame. Give it a [name](Self::name) for assistive technology, and a
    /// [placeholder](Self::placeholder) if it's empty at first.
    #[must_use]
    pub fn field(key: Key, value: &'s str, on_edit: fn(TextEdit) -> M) -> Self {
        Self {
            on_edit: Some(on_edit),
            // A text box: a surface inside a border.
            look: Look {
                fill: Some(Tone::Surface),
                border: Some(Tone::Border),
                radius: Radius::Control,
                ..Look::PLAIN
            },
            ..Self::new(key, Role::Field, value, None)
        }
    }

    /// A field whose text can be selected and copied, but not changed.
    #[must_use]
    pub fn read_only(self) -> Self {
        Self {
            on_edit: None,
            ..self
        }
    }

    /// What a field shows, in secondary text, while it's empty; UI Automation's help text then.
    #[must_use]
    pub fn placeholder(self, placeholder: &'s str) -> Self {
        Self {
            placeholder,
            ..self
        }
    }

    /// The message Enter sends while the field has focus (a palette runs its command).
    #[must_use]
    pub fn submit(self, msg: M) -> Self {
        Self {
            msg: Some(msg),
            ..self
        }
    }

    #[must_use]
    pub fn style(self, style: Style) -> Self {
        Self { style, ..self }
    }

    #[must_use]
    pub fn text_style(self, text_style: TextStyle) -> Self {
        Self { text_style, ..self }
    }

    #[must_use]
    pub fn look(self, look: Look) -> Self {
        Self { look, ..self }
    }

    /// A disabled node takes no input and no focus.
    #[must_use]
    pub fn enabled(self, enabled: bool) -> Self {
        Self { enabled, ..self }
    }

    /// Makes the node an item of a set the user picks from (a table row, a rail page), currently
    /// picked or not: UI Automation's selection item. The view still chooses its look.
    #[must_use]
    pub fn selected(self, selected: bool) -> Self {
        Self {
            selected: Some(selected),
            ..self
        }
    }

    /// The accessible name of a container, which draws no text ("Pages" for a rail, a table's
    /// title), or of a field, whose text is its value. Labels and buttons are named by their text.
    #[must_use]
    pub fn name(self, name: &'s str) -> Self {
        if self.role == Role::Field {
            Self {
                label: name,
                ..self
            }
        } else {
            Self { text: name, ..self }
        }
    }
}

/// No node: the end of a sibling list, or no parent.
pub(crate) const NIL: u32 = u32::MAX;

/// A node in the arena. Links are arena indices, so moving a node is O(1) and allocates nothing.
#[derive(Debug)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent per-node flags, read on hot paths"
)]
pub(crate) struct Node<M> {
    pub(crate) generation: u32,
    pub(crate) live: bool,
    pub(crate) key: Key,
    pub(crate) role: Role,
    pub(crate) style: Style,
    /// From the window's [`TextPool`](crate::pool::TextPool), and back to it when freed.
    pub(crate) text: String,
    pub(crate) text_style: TextStyle,
    pub(crate) look: Look,
    pub(crate) msg: Option<M>,
    pub(crate) check: Tri,
    pub(crate) enabled: bool,
    pub(crate) selected: Option<bool>,
    /// A field's slot in the Ui's field table; [`NIL`] for other roles.
    pub(crate) field: u32,

    pub(crate) parent: u32,
    pub(crate) first: u32,
    pub(crate) last: u32,
    pub(crate) prev: u32,
    pub(crate) next: u32,

    /// Where the last layout put the node.
    pub(crate) rect: Rect,
    /// This node or a descendant changed since the last layout.
    pub(crate) dirty: bool,
    /// `measured` is the size for an available width of `measured_for`.
    pub(crate) cache_valid: bool,
    pub(crate) measured_for: f32,
    pub(crate) measured: Size,
    /// Scratch for the parent's layout pass: the size along its axis.
    pub(crate) main: f32,

    pub(crate) hovered: bool,
    pub(crate) pressed: bool,
    /// The hover highlight, 0 to 1, animated toward `hovered`.
    pub(crate) hover_t: f32,
    pub(crate) anim_from: f32,
    /// When the running animation started; `None` until its first tick.
    pub(crate) anim_start: Option<u64>,
    /// In the window's animation set.
    pub(crate) animating: bool,
    /// Listed for the next accessibility update.
    pub(crate) a11y_dirty: bool,
}

impl<M> Node<M> {
    pub(crate) fn new(key: Key, role: Role) -> Self {
        Self {
            generation: 0,
            live: true,
            key,
            role,
            style: Style::default(),
            text: String::new(),
            text_style: TextStyle::default(),
            look: Look::PLAIN,
            msg: None,
            check: Tri::Off,
            enabled: true,
            selected: None,
            field: NIL,
            parent: NIL,
            first: NIL,
            last: NIL,
            prev: NIL,
            next: NIL,
            rect: Rect::default(),
            dirty: true,
            cache_valid: false,
            measured_for: f32::NAN,
            measured: Size::ZERO,
            main: 0.0,
            hovered: false,
            pressed: false,
            hover_t: 0.0,
            anim_from: 0.0,
            anim_start: None,
            animating: false,
            a11y_dirty: false,
        }
    }

    /// Makes a freed node new again under `key` with the text buffer `text`, keeping its
    /// generation.
    pub(crate) fn reuse(&mut self, key: Key, role: Role, text: String) {
        let generation = self.generation;
        *self = Self::new(key, role);
        self.text = text;
        self.generation = generation;
    }

    pub(crate) fn interactive(&self) -> bool {
        self.enabled && self.role.interactive()
    }
}

/// A read-only view of a node, for tests, probes and the accessibility tree.
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "a snapshot of independent flags"
)]
pub struct NodeInfo<'a> {
    pub role: Role,
    pub key: Key,
    pub rect: Rect,
    pub text: &'a str,
    pub enabled: bool,
    pub check: Tri,
    /// Whether it's picked, for an item of a set the user picks from.
    pub selected: Option<bool>,
    pub hovered: bool,
    pub pressed: bool,
    pub focused: bool,
    /// The animated hover highlight, 0 to 1.
    pub hover: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_stable_and_distinct() {
        const HEADER: Key = Key::name("header");
        assert_eq!(HEADER, Key::name("header"));
        assert_ne!(Key::name("header"), Key::name("headex"));
        assert_ne!(Key::index(0), Key::index(1));
        assert_ne!(Key::index(0), Key(0));
        // FNV-1a's published vector for "a".
        assert_eq!(Key::name("a").0, 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn only_enabled_buttons_checkboxes_and_fields_are_interactive() {
        let mut n: Node<()> = Node::new(Key(1), Role::Button);
        assert!(n.interactive());
        n.enabled = false;
        assert!(!n.interactive());
        assert!(!Node::<()>::new(Key(1), Role::Label).interactive());
        assert!(Node::<()>::new(Key(1), Role::CheckBox).interactive());
        assert!(Node::<()>::new(Key(1), Role::Field).interactive());
    }
}
