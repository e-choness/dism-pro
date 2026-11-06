//! The retained tree's pieces: keys, node ids, roles, the per-frame [`Element`] description, and
//! the long-lived node behind it (TD-0009 §5.1).

use dpro_app::table::Tri;

use crate::{
    field::TextEdit,
    geom::{Insets, Rect, Size},
    style::{Axis, Dim, Style},
    text::TextStyle,
    theme::{
        Tone,
        tokens::{font, popup, radius},
    },
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
    /// One of a [tab list](Group::Tabs)'s tabs: picked or not, and its message picks it.
    Tab,
    /// One of a [radio group](Group::Radios)'s options: picked or not, and its message picks it.
    Radio,
    /// How far a job has come, as a bar: a fraction from 0 to 1. Not interactive.
    Progress,
    /// A [menu](Element::menu)'s item, or a dropdown list's [option](Element::option): its
    /// message invokes or picks it. Hovering it focuses it.
    MenuItem,
    /// A [dropdown](Element::combo): shows its value, and its message opens its list.
    ComboBox,
    /// A [line](Element::separator) between a menu's groups of items. Not interactive.
    Separator,
    /// A tooltip window's [text](Element::tooltip_text). Not interactive.
    Tooltip,
}

impl Role {
    pub(crate) fn interactive(self) -> bool {
        matches!(
            self,
            Self::Button
                | Self::CheckBox
                | Self::Field
                | Self::Tab
                | Self::Radio
                | Self::MenuItem
                | Self::ComboBox
        )
    }
}

/// A container whose items the arrow keys move through (TD-0009 §4.3): the whole group is one Tab
/// stop, which lands on its picked item (or its first), and Home and End go to its ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    /// [Tabs](Element::tab), in a row: Left and Right move focus and pick, wrapping.
    Tabs,
    /// [Radio buttons](Element::radio): every arrow key moves focus and picks, wrapping, as
    /// Windows' radio buttons do.
    Radios,
    /// Items the arrows move focus through along the container's axis, without picking or
    /// wrapping (a navigation rail): Enter or Space opens one.
    List,
    /// A [menu](Element::menu)'s items: Up and Down move focus, wrapping, as Windows menus do;
    /// Enter or Space invokes. A disabled item can be focused, not invoked.
    Menu,
    /// A dropdown list's [options](Element::options): Up and Down move focus without wrapping,
    /// and Enter or Space picks.
    Options,
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

    /// The primary button (doc 05; artboard 06): the accent, text on it. Give it semibold text.
    pub const PRIMARY: Self = Self {
        fill: Some(Tone::Accent),
        border: None,
        radius: Radius::Control,
        ink: Tone::OnAccent,
    };

    /// A secondary button: the quiet control layer (`layer.control` on glass) in a hairline.
    pub const SECONDARY: Self = Self {
        fill: Some(Tone::Subtle),
        border: Some(Tone::Border),
        radius: Radius::Control,
        ink: Tone::Text,
    };

    /// A danger button, as Hold to apply is: the danger color, text on it. Give it semibold text.
    pub const DANGER: Self = Self {
        fill: Some(Tone::Danger),
        border: None,
        radius: Radius::Control,
        ink: Tone::OnAccent,
    };

    /// A button that is only its text until hovered or pressed (caption buttons, icon buttons).
    pub const SUBTLE: Self = Self {
        fill: None,
        border: None,
        radius: Radius::Control,
        ink: Tone::Text,
    };

    /// Artboard 08's segmented tab list: the tabs on a quiet track (ink at 6 %).
    pub const TABS: Self = Self {
        fill: Some(Tone::Hover),
        border: None,
        radius: Radius::Control,
        ink: Tone::Text,
    };

    /// A tab not picked: secondary text on the track.
    pub const TAB: Self = Self {
        fill: None,
        border: None,
        radius: Radius::Control,
        ink: Tone::TextSecondary,
    };

    /// The picked tab: a surface in a hairline (the canvas's raised segment), in primary text.
    /// Give it semibold text.
    pub const TAB_SELECTED: Self = Self {
        fill: Some(Tone::Surface),
        border: Some(Tone::Border),
        radius: Radius::Control,
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
    /// Its [tooltip](Self::tooltip), empty for none.
    pub(crate) tip: &'s str,
    /// Popup state few nodes have, as [`rare`] flags: a tooltip, a context menu, whether what it
    /// opens is open. Building a node with none tests this one byte and skips the rest.
    pub(crate) rare: u8,
    /// What only its role describes.
    pub(crate) extra: Extra<'s, M>,
}

/// [`Element::rare`]'s flags, which a node keeps as they were last built. (Measured on the shell
/// fixture: as separate fields compared for every node, rebuilding a view in which nothing changed
/// cost 9.5 % more.)
pub(crate) mod rare {
    /// A [context](super::Element::context) request on it, or inside it, asks for the model's menu.
    pub(crate) const CONTEXT: u8 = 1;
    /// It opens a popup (a dropdown, a menu button), so a client sees whether it's open.
    pub(crate) const EXPANDABLE: u8 = 2;
    /// What it opens is open.
    pub(crate) const EXPANDED: u8 = 4;
    /// It has a [tooltip](super::Element::tooltip).
    pub(crate) const TIP: u8 = 8;

    /// Whether what a node with flags `r` opens is open; `None` if it opens nothing.
    pub(crate) const fn expanded(r: u8) -> Option<bool> {
        if r & EXPANDABLE == 0 {
            None
        } else {
            Some(r & EXPANDED != 0)
        }
    }

    /// The flags of a node that opens a popup, open if `open`.
    pub(crate) const fn expandable(r: u8, open: bool) -> u8 {
        let r = r & !EXPANDED | EXPANDABLE;
        if open { r | EXPANDED } else { r }
    }
}

/// What only some roles describe. One at a time, so an [`Element`] carries one field for all of
/// them, and building a node of any other role skips them all.
#[derive(Debug)]
pub(crate) enum Extra<'s, M> {
    None,
    /// A field's accessible name, what it shows while it's empty, and its edits as the view's
    /// messages (`None` for a read-only field).
    Field {
        label: &'s str,
        placeholder: &'s str,
        on_edit: Option<fn(TextEdit) -> M>,
    },
    /// A container whose items the arrow keys move through.
    Group(Group),
    /// A progress bar's fraction, 0 to 1.
    Progress(f32),
    /// What UI Automation's Invoke sends for a [hold](Element::hold) button, instead of its
    /// message.
    Hold(fn() -> M),
    /// A dropdown's accessible name, and what Up and Down send while it's closed: the next
    /// option (`true`) or the one before.
    Combo {
        label: &'s str,
        step: Option<fn(bool) -> M>,
    },
}

// Copy whatever `M` is: it holds no `M`, only functions that make one, so building a node drops
// nothing of it (measured on the shell fixture: with an `M` here, rebuilding a view in which
// nothing changed cost 7 % more).
impl<M> Clone for Extra<'_, M> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<M> Copy for Extra<'_, M> {}

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
            tip: "",
            rare: 0,
            extra: Extra::None,
        }
    }

    /// A container for children.
    #[must_use]
    pub fn stack(key: Key) -> Self {
        Self::new(key, Role::Container, "", None)
    }

    /// A container for [tabs](Self::tab), in a row: one Tab stop, Left and Right pick
    /// ([`Group::Tabs`]). Name it.
    #[must_use]
    pub fn tabs(key: Key) -> Self {
        Self {
            extra: Extra::Group(Group::Tabs),
            style: Style {
                axis: Axis::Row,
                ..Style::default()
            },
            ..Self::stack(key)
        }
    }

    /// A container for [radio buttons](Self::radio): one Tab stop, the arrow keys pick
    /// ([`Group::Radios`]). Name it.
    #[must_use]
    pub fn radios(key: Key) -> Self {
        Self {
            extra: Extra::Group(Group::Radios),
            ..Self::stack(key)
        }
    }

    /// A container for [selectable](Self::selected) buttons, a navigation rail's pages: one Tab
    /// stop, the arrow keys move focus along its axis, Enter or Space opens ([`Group::List`]).
    /// Name it.
    #[must_use]
    pub fn list(key: Key) -> Self {
        Self {
            extra: Extra::Group(Group::List),
            ..Self::stack(key)
        }
    }

    /// A menu (TD-0009 A1): the content of a menu popup, a column of
    /// [items](Self::menu_item) and [separators](Self::separator). It's one Tab stop, and Up and
    /// Down move through its items, wrapping ([`Group::Menu`]). The surface in a hairline,
    /// padded; the window's corners are DWM's.
    #[must_use]
    pub fn menu(key: Key) -> Self {
        Self {
            extra: Extra::Group(Group::Menu),
            style: Style {
                padding: Insets::all(popup::PADDING),
                ..Style::default()
            },
            look: POPUP,
            ..Self::stack(key)
        }
    }

    /// A menu item; `msg` invokes it, from a click, Enter, Space or UI Automation's Invoke, and
    /// the host closes the menu after. Hovering it focuses it. Disabled, it can still be focused
    /// from the keyboard, and [tooltip](Self::tooltip) text can say why it's disabled. With
    /// children (its label and its accelerator), `label` is only its accessible name.
    #[must_use]
    pub fn menu_item(key: Key, label: &'s str, msg: M) -> Self {
        Self {
            style: item_style(),
            look: Look {
                radius: Radius::Small,
                ..Look::PLAIN
            },
            ..Self::new(key, Role::MenuItem, label, Some(msg))
        }
    }

    /// A hairline between a menu's groups of items, 4 px from each.
    #[must_use]
    pub fn separator(key: Key) -> Self {
        Self {
            style: Style {
                height: Dim::Fixed(popup::SEPARATOR),
                ..Style::default()
            },
            ..Self::new(key, Role::Separator, "", None)
        }
    }

    /// A dropdown's list: the content of a list popup, a column of [options](Self::option).
    /// It's one Tab stop, Up and Down move focus without wrapping, and Enter or Space picks
    /// ([`Group::Options`]). Looks as a [menu](Self::menu) does.
    #[must_use]
    pub fn options(key: Key) -> Self {
        Self {
            extra: Extra::Group(Group::Options),
            ..Self::menu(key)
        }
    }

    /// One of a dropdown list's options, `selected` (the value now) or not; `msg` picks it, and
    /// the host closes the list after. Give the selected one [`Tone::Selected`] if it should show.
    #[must_use]
    pub fn option(key: Key, label: &'s str, selected: bool, msg: M) -> Self {
        Self {
            selected: Some(selected),
            ..Self::menu_item(key, label, msg)
        }
    }

    /// A dropdown (UI Automation's combo box), showing `value` with a chevron at its end and
    /// named `label`; `msg` asks the model to open its list, from a click, Enter, Space, Alt+Down,
    /// F4 or UI Automation's Expand. `expanded` says whether the list is open. Up and Down change
    /// the value without opening it once given a [step](Self::step).
    #[must_use]
    pub fn combo(key: Key, label: &'s str, value: &'s str, expanded: bool, msg: M) -> Self {
        Self {
            extra: Extra::Combo { label, step: None },
            rare: rare::expandable(0, expanded),
            style: item_style(),
            look: Look::SECONDARY,
            ..Self::new(key, Role::ComboBox, value, Some(msg))
        }
    }

    /// What Up and Down send while a [dropdown](Self::combo) is closed and focused: `step(true)`
    /// for the next option, `step(false)` for the one before. Other roles ignore it.
    #[must_use]
    pub fn step(mut self, on_step: fn(bool) -> M) -> Self {
        if let Extra::Combo { step, .. } = &mut self.extra {
            *step = Some(on_step);
        }
        self
    }

    /// A tooltip window's text, in caption type, wrapping at
    /// [`TIP_MAX_W`](crate::tokens::popup::TIP_MAX_W): what the host shows for
    /// [`Ui::tooltip`](crate::Ui::tooltip).
    #[must_use]
    pub fn tooltip_text(key: Key, text: &'s str) -> Self {
        Self {
            style: Style {
                padding: Insets::xy(popup::TIP_PAD_X, popup::TIP_PAD_Y),
                max: Size {
                    w: popup::TIP_MAX_W,
                    h: f32::INFINITY,
                },
                ..Style::default()
            },
            text_style: TextStyle {
                size: font::CAPTION,
                wrap: true,
                ..TextStyle::default()
            },
            look: POPUP,
            ..Self::new(key, Role::Tooltip, text, None)
        }
    }

    /// A tab in a [tab list](Self::tabs), `selected` or not; `msg` picks it, from a click,
    /// Enter, Space, the arrow keys or UI Automation's Select. The view chooses its look
    /// ([`Look::TAB`], [`Look::TAB_SELECTED`]).
    #[must_use]
    pub fn tab(key: Key, label: &'s str, selected: bool, msg: M) -> Self {
        Self {
            selected: Some(selected),
            look: Look::TAB,
            ..Self::new(key, Role::Tab, label, Some(msg))
        }
    }

    /// A radio button in a [radio group](Self::radios), `selected` or not; `msg` picks it, as
    /// for a [tab](Self::tab). Drawn as a circle a line high, then its label.
    #[must_use]
    pub fn radio(key: Key, label: &'s str, selected: bool, msg: M) -> Self {
        Self {
            selected: Some(selected),
            ..Self::new(key, Role::Radio, label, Some(msg))
        }
    }

    /// A progress bar, `fraction` (0 to 1, clamped) of the way: a pill-shaped track as wide and
    /// high as its style makes it, filled from the start in the accent. Name it; UI Automation
    /// reads it as a range value from 0 to 100.
    #[must_use]
    pub fn progress(key: Key, fraction: f32) -> Self {
        Self {
            extra: Extra::Progress(if fraction.is_nan() {
                0.0
            } else {
                fraction.clamp(0.0, 1.0)
            }),
            ..Self::new(key, Role::Progress, "", None)
        }
    }

    /// Makes a button a hold button (TD-0009 §5.2, doc 05 §4): its message is sent only after a
    /// continuous press of [`motion::HOLD_MS`](crate::tokens::motion::HOLD_MS), by the pointer
    /// held on it, or Enter or Space held while it has focus, timed on the frame clock. Letting
    /// go earlier sends nothing. While held, it fills from its start toward its end. UI
    /// Automation's Invoke, which can't hold, sends `invoke()` instead: the typed confirmation
    /// (§8). For danger buttons ([`Look::DANGER`]); other roles ignore it.
    #[must_use]
    pub fn hold(mut self, invoke: fn() -> M) -> Self {
        if self.role == Role::Button {
            self.extra = Extra::Hold(invoke);
        }
        self
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
            extra: Extra::Field {
                label: "",
                placeholder: "",
                on_edit: Some(on_edit),
            },
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
    pub fn read_only(mut self) -> Self {
        if let Extra::Field { on_edit, .. } = &mut self.extra {
            *on_edit = None;
        }
        self
    }

    /// What a field shows, in secondary text, while it's empty; UI Automation's help text then.
    #[must_use]
    pub fn placeholder(mut self, text: &'s str) -> Self {
        if let Extra::Field { placeholder, .. } = &mut self.extra {
            *placeholder = text;
        }
        self
    }

    /// The message Enter sends while the field has focus (a palette runs its command).
    #[must_use]
    pub fn submit(mut self, msg: M) -> Self {
        self.msg = Some(msg);
        self
    }

    // The setters assign a field of `self` rather than build `Self { field, ..self }`, which
    // copied the whole element for every call (measured on the shell fixture: 20 Ir a `look`).

    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    #[must_use]
    pub fn text_style(mut self, text_style: TextStyle) -> Self {
        self.text_style = text_style;
        self
    }

    #[must_use]
    pub fn look(mut self, look: Look) -> Self {
        self.look = look;
        self
    }

    /// A disabled node takes no input and no focus.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Makes the node an item of a set the user picks from (a table row, a rail page), currently
    /// picked or not: UI Automation's selection item. The view still chooses its look.
    #[must_use]
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = Some(selected);
        self
    }

    /// The accessible name of a container, which draws no text ("Pages" for a rail, a table's
    /// title), or of a field, whose text is its value. Labels and buttons are named by their text.
    #[must_use]
    pub fn name(mut self, name: &'s str) -> Self {
        match &mut self.extra {
            Extra::Field { label, .. } | Extra::Combo { label, .. } => *label = name,
            _ => self.text = name,
        }
        self
    }

    /// Text the toolkit shows in a tooltip while the pointer rests on the node, or at once when
    /// the keyboard focuses it ([`Ui::tooltip`](crate::Ui::tooltip)); assistive technology reads
    /// it as the node's description, whether it shows or not. A command's label and its
    /// accelerator, or why it's disabled. Disabled nodes show theirs too.
    #[must_use]
    pub fn tooltip(mut self, text: &'s str) -> Self {
        self.tip = text;
        self.rare = if text.is_empty() {
            self.rare & !rare::TIP
        } else {
            self.rare | rare::TIP
        };
        self
    }

    /// A context request (a right-click, Shift+F10, the Menu key) on this node or inside it
    /// focuses it, picks it if it's [selectable](Self::selected), and asks the model for its
    /// context menu (`Model::context_menu`).
    #[must_use]
    pub fn context(mut self) -> Self {
        self.rare |= rare::CONTEXT;
        self
    }

    /// Says whether the popup this node opens is open (a menu button): UI Automation's
    /// `ExpandCollapse`. A [dropdown](Self::combo) has it already.
    #[must_use]
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.rare = rare::expandable(self.rare, expanded);
        self
    }
}

/// A menu's and a list's look: the surface in a hairline. Under glass the surface is a
/// translucent layer, so the window's acrylic shows through.
const POPUP: Look = Look {
    fill: Some(Tone::Surface),
    border: Some(Tone::Border),
    radius: Radius::None,
    ink: Tone::Text,
};

/// A menu item's, an option's and a dropdown's style: at least an item high, text 12 px from
/// its sides.
fn item_style() -> Style {
    Style {
        min: Size {
            w: 0.0,
            h: popup::ITEM_HEIGHT,
        },
        padding: Insets::xy(popup::ITEM_PAD, 0.0),
        ..Style::default()
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
    /// The arrow keys move through this container's items.
    pub(crate) group: Option<Group>,
    /// A progress bar's fraction, 0 to 1.
    pub(crate) value: f32,
    /// A hold button: its Invoke message is in the Ui's hold table.
    pub(crate) hold: bool,
    /// It has a group, a progress value, a hold or a dropdown's step, which an element without
    /// them clears.
    pub(crate) special: bool,
    /// Its slot in the Ui's side table (a tooltip, a dropdown's name and step); [`NIL`] if none.
    pub(crate) side: u32,
    /// Whether what it opens is open; `None` if it opens nothing.
    pub(crate) expanded: Option<bool>,
    /// Context requests on it or inside it ask for the model's menu.
    pub(crate) context: bool,
    /// Its element's [`rare`] flags as last built: with these and the new element's both 0, an
    /// update skips the popup state.
    pub(crate) rare: u8,

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
            group: None,
            value: 0.0,
            hold: false,
            special: false,
            side: NIL,
            expanded: None,
            context: false,
            rare: 0,
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

    /// The arrow keys can bring focus to it: an interactive node, or a disabled menu item, which
    /// Windows menus let the keyboard reach (TD-0009 A1.4).
    pub(crate) fn focusable(&self) -> bool {
        self.interactive() || self.role == Role::MenuItem
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
    /// The style its text is measured and drawn in: the view's, times Windows' text size.
    pub text_style: TextStyle,
    pub enabled: bool,
    pub check: Tri,
    /// Whether it's picked, for an item of a set the user picks from.
    pub selected: Option<bool>,
    /// Whether what it opens is open, for a node that opens a popup.
    pub expanded: Option<bool>,
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
    fn only_enabled_controls_are_interactive() {
        let mut n: Node<()> = Node::new(Key(1), Role::Button);
        assert!(n.interactive());
        n.enabled = false;
        assert!(!n.interactive());
        for role in [
            Role::Container,
            Role::Label,
            Role::Progress,
            Role::Separator,
            Role::Tooltip,
        ] {
            assert!(!Node::<()>::new(Key(1), role).interactive(), "{role:?}");
        }
        for role in [
            Role::CheckBox,
            Role::Field,
            Role::Tab,
            Role::Radio,
            Role::MenuItem,
            Role::ComboBox,
        ] {
            assert!(Node::<()>::new(Key(1), role).interactive(), "{role:?}");
        }
        // A disabled menu item can still be reached from the keyboard; a disabled button can't.
        let mut item: Node<()> = Node::new(Key(1), Role::MenuItem);
        item.enabled = false;
        assert!(item.focusable() && !item.interactive());
        n.enabled = false;
        assert!(!n.focusable());
    }

    #[test]
    fn progress_is_a_clamped_fraction() {
        let value = |f: f32| match Element::<()>::progress(Key(1), f).extra {
            Extra::Progress(v) => v,
            _ => f32::NAN,
        };
        assert_eq!(
            [value(-1.0), value(0.41), value(2.0), value(f32::NAN)],
            [0.0, 0.41, 1.0, 0.0]
        );
    }
}
