//! Text fields (TD-0009 §11 step 3): one line of editable text, with a caret, a selection, the
//! clipboard and UI Automation's Value pattern.
//!
//! **The view owns the value.** A field shows the text its view gives it. Each edit is applied to
//! the field at once, so the next keystroke builds on it, and reaches the view as a message made
//! by the view's `on_edit` from a [`TextEdit`]. The view applies the edit to its copy
//! ([`TextEdit::apply`]) and gives the result back next frame, which then matches and copies
//! nothing. A view that keeps something else (it rejected or rewrote the edit) wins: the field
//! takes the view's text, with the caret at its end. Typing, deleting and moving the caret
//! allocate nothing (U8); a paste or a value set through UI Automation allocates its text once.
//!
//! **What a field holds.** At most [`MAX_FIELD_LEN`] bytes, the cap TD-0009 §8 puts on any text.
//! No control characters, line breaks, or bidi overrides and isolates (U+202A–U+202E,
//! U+2066–U+2069; §8 "Trojan Source"): typing them inserts nothing. A paste drops line breaks, as
//! §8 asks of single-line fields, turns tabs into spaces, and drops the rest; text past the cap is
//! cut at a character boundary.
//!
//! **Caret stops.** The caret sits only where the [`TextSystem`] puts a caret stop: at cluster
//! boundaries, so it never splits a base character from its combining marks or a surrogate pair.
//! Layout gets the stops of a field whose text changed. Movement is in logical (reading) order.
//!
//! **Keys,** while a field has focus (Windows' edit control):
//! - Left and Right move a stop, with Ctrl a word; Home and End go to the ends. Shift extends the
//!   selection; without it a selection collapses to the side moved toward.
//! - Backspace and Delete remove the selection, else a stop (with Ctrl, a word) before or after.
//! - Ctrl+A selects all. Ctrl+C or Ctrl+Insert copies, Ctrl+X or Shift+Delete cuts, Ctrl+V or
//!   Shift+Insert pastes: the field asks the platform through [`Ui::take_clipboard`].
//! - Enter sends the field's [submit](crate::Element::submit) message, if it has one.
//! - Space comes as a character. Tab moves focus; Esc, Up, Down, Page Up and Page Down go to the
//!   caller (a palette moves its selection with them).
//!
//! Tab into a field selects all of it; a click puts the caret under the pointer, and dragging
//! selects. A word is a run of letters, digits and underscores, or of other non-space
//! characters.
//!
//! **The caret** shows while the window is active and a field has focus, [`Caret::width`] pixels
//! wide at 100 %. It blinks at [`Caret::blink_ms`] for [`Caret::BLINK_FOR_MS`] after the last
//! keystroke or caret move, then stays on and its timer stops, so a settled window wakes for
//! nothing (U6). The selection shows only while the window is active and the field has focus.

use crate::{
    geom::{Point, Rect},
    text::{CaretStop, TextSystem},
    tree::{NIL, NodeId, Role},
    ui::Ui,
};

/// The most a field holds: 64 KiB of UTF-8, TD-0009 §8's cap on any text.
pub const MAX_FIELD_LEN: usize = 64 * 1024;

/// One change to a field's text: the bytes `start..end` of the text before it replaced by
/// `insert`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEdit {
    pub start: u32,
    pub end: u32,
    pub insert: Insert,
}

/// What an edit inserts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Insert {
    /// Nothing: a deletion.
    Nothing,
    /// One typed character.
    Char(char),
    /// Pasted text, or a value set through UI Automation.
    Text(Box<str>),
}

impl Insert {
    fn as_str<'b>(&'b self, buf: &'b mut [u8; 4]) -> &'b str {
        match self {
            Self::Nothing => "",
            Self::Char(c) => c.encode_utf8(buf),
            Self::Text(t) => t,
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::Nothing => 0,
            Self::Char(c) => c.len_utf8(),
            Self::Text(t) => t.len(),
        }
    }
}

impl TextEdit {
    /// Applies the edit to `text`, the view's copy of the field's value. Returns false and leaves
    /// `text` alone if the range isn't in `text` at character boundaries: `text` wasn't the
    /// field's value. Allocates only if `text` must grow.
    pub fn apply(&self, text: &mut String) -> bool {
        let (start, end) = (self.start as usize, self.end as usize);
        if start > end || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            return false;
        }
        let mut buf = [0; 4];
        text.replace_range(start..end, self.insert.as_str(&mut buf));
        true
    }
}

/// A copy, cut or paste a key in a field asked for. The platform carries it out with the
/// clipboard: for [`Copy`](Self::Copy), it writes [`Ui::selected_text`]; for [`Cut`](Self::Cut),
/// the same, then [`Ui::cut_selection`] if the write worked; for [`Paste`](Self::Paste), it reads
/// the clipboard's text into [`Ui::paste`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardRequest {
    Copy,
    Cut,
    Paste,
}

/// The pointer's shape over what it's on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Cursor {
    #[default]
    Arrow,
    /// An I-beam, over a field.
    Text,
}

/// How the caret looks and blinks: Windows' settings (`SPI_GETCARETWIDTH`, `GetCaretBlinkTime`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caret {
    /// In pixels at 100 % scale; scaled with the window, and at least one pixel.
    pub width: u32,
    /// How long the caret is on, then off; 0 doesn't blink (the setting's "None").
    pub blink_ms: u32,
}

impl Caret {
    /// Windows' defaults.
    pub const WINDOWS: Self = Self {
        width: 1,
        blink_ms: 530,
    };

    /// How long the caret blinks after the last keystroke, caret move or focus change. Then it
    /// stays on and asks for no more wakes (U6). WCAG 2.2.2 asks blinking to stop within 5 s; GTK
    /// stops a caret's after 10 s.
    pub const BLINK_FOR_MS: u32 = 5_000;
}

impl Default for Caret {
    fn default() -> Self {
        Self::WINDOWS
    }
}

/// A field's caret and selection, for tests and probes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FieldInfo {
    /// Byte offset of the caret: the selection's moving end.
    pub caret: u32,
    /// Byte offset of the selection's fixed end; equal to `caret` when nothing is selected.
    pub anchor: u32,
    /// How far the text is scrolled left, in logical pixels, to keep the caret in view.
    pub scroll: f32,
}

/// The state of one field node, in the Ui's field table: the node itself keeps the text.
#[derive(Debug)]
pub(crate) struct Field<M> {
    /// The node, while the slot is in use.
    pub(crate) node: u32,
    pub(crate) live: bool,
    /// The accessible name.
    pub(crate) name: String,
    /// Shown while the field is empty; UI Automation's help text then.
    pub(crate) placeholder: String,
    /// `None`: read-only. The text can be selected and copied, not changed.
    pub(crate) on_edit: Option<fn(TextEdit) -> M>,
    pub(crate) caret: u32,
    pub(crate) anchor: u32,
    pub(crate) scroll: f32,
    /// The caret stops of the text, in logical order, from the last layout.
    pub(crate) stops: Vec<CaretStop>,
    /// `stops` belong to the current text.
    pub(crate) stops_valid: bool,
    /// The widest stop: the text's width.
    pub(crate) width: f32,
    /// The stops' x only grows (no right-to-left run), so the visible part can be found by x.
    pub(crate) ordered: bool,
    /// A line's height in the field's text style.
    pub(crate) line: f32,
    /// The content box's width at the last layout.
    pub(crate) view: f32,
}

impl<M> Field<M> {
    fn new(node: u32) -> Self {
        Self {
            node,
            live: true,
            name: String::new(),
            placeholder: String::new(),
            on_edit: None,
            caret: 0,
            anchor: 0,
            scroll: 0.0,
            stops: Vec::new(),
            stops_valid: false,
            width: 0.0,
            ordered: true,
            line: 0.0,
            view: 0.0,
        }
    }

    /// The line the text sits on in `content`, the content box: as wide, a line high, centered.
    pub(crate) fn line_box(&self, content: Rect) -> Rect {
        Rect::new(
            content.x,
            content.y + ((content.h - self.line) / 2.0).max(0.0),
            content.w,
            self.line,
        )
    }

    /// The part of `text`, the field's, that shows, and its x from the text's start: from the stop
    /// at or before the left edge to the one at or after the right. All of it when the stops are
    /// stale or not ordered by x.
    pub(crate) fn visible<'t>(&self, text: &'t str) -> (&'t str, f32) {
        if !self.stops_valid || !self.ordered || self.stops.is_empty() {
            return (text, 0.0);
        }
        let left = self
            .stops
            .partition_point(|s| s.x <= self.scroll)
            .saturating_sub(1);
        let right = self
            .stops
            .partition_point(|s| s.x < self.scroll + self.view)
            .min(self.stops.len() - 1);
        let (a, b) = (self.stops[left], self.stops[right]);
        text.get(a.index as usize..b.index as usize)
            .map_or((text, 0.0), |slice| (slice, a.x))
    }

    /// The selection as an ordered byte range.
    pub(crate) fn selection(&self) -> (usize, usize) {
        let (a, b) = (self.anchor as usize, self.caret as usize);
        (a.min(b), a.max(b))
    }

    /// The x of the caret stop at byte `index`, from the text's start; the nearest stop before it
    /// if the stops are stale.
    pub(crate) fn x_of(&self, index: usize) -> f32 {
        let k = self.stops.partition_point(|s| (s.index as usize) <= index);
        k.checked_sub(1).map_or(0.0, |k| self.stops[k].x)
    }

    /// The stop nearest `x` (from the text's start), clamped into `text`.
    fn hit(&self, text: &str, x: f32) -> usize {
        let best = self
            .stops
            .iter()
            .filter(|s| (s.index as usize) <= text.len())
            .min_by(|a, b| (a.x - x).abs().total_cmp(&(b.x - x).abs()))
            .map_or(0, |s| s.index as usize);
        floor_boundary(text, best)
    }

    /// The stop after byte `at`, or the end.
    fn next_stop(&self, text: &str, at: usize) -> usize {
        if !self.stops_valid {
            return text[at..]
                .chars()
                .next()
                .map_or(text.len(), |c| at + c.len_utf8());
        }
        let k = self.stops.partition_point(|s| (s.index as usize) <= at);
        self.stops.get(k).map_or(text.len(), |s| s.index as usize)
    }

    /// The stop before byte `at`, or the start.
    fn prev_stop(&self, text: &str, at: usize) -> usize {
        if !self.stops_valid {
            return text[..at].char_indices().next_back().map_or(0, |(k, _)| k);
        }
        let k = self.stops.partition_point(|s| (s.index as usize) < at);
        k.checked_sub(1).map_or(0, |k| self.stops[k].index as usize)
    }

    /// `at` moved forward to a stop, if it isn't one.
    fn stop_at_or_after(&self, text: &str, at: usize) -> usize {
        if !self.stops_valid {
            return at;
        }
        let k = self.stops.partition_point(|s| (s.index as usize) < at);
        self.stops.get(k).map_or(text.len(), |s| s.index as usize)
    }

    /// `at` moved back to a stop, if it isn't one.
    fn stop_at_or_before(&self, at: usize) -> usize {
        if !self.stops_valid {
            return at;
        }
        let k = self.stops.partition_point(|s| (s.index as usize) <= at);
        k.checked_sub(1).map_or(0, |k| self.stops[k].index as usize)
    }

    /// Scrolls as little as keeps the caret, `caret_w` wide, inside the content box, and no
    /// further than the text needs. Returns whether the scroll changed.
    fn fit_scroll(&mut self, caret_w: f32) -> bool {
        let before = self.scroll;
        let max = (self.width + caret_w - self.view).max(0.0);
        let x = self.x_of(self.caret as usize);
        let mut s = self.scroll.clamp(0.0, max);
        if x + caret_w - s > self.view {
            s = x + caret_w - self.view;
        }
        if x < s {
            s = x;
        }
        self.scroll = s.clamp(0.0, max);
        self.scroll.to_bits() != before.to_bits()
    }
}

/// What a field may hold (see the module docs).
fn allowed(c: char) -> bool {
    !c.is_control() && !matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// `text` as a single-line field takes it, at most `room` bytes: line breaks dropped, tabs as
/// spaces, other characters a field can't hold dropped, cut at a character boundary.
fn single_line(text: &str, room: usize) -> String {
    let mut out = String::with_capacity(text.len().min(room));
    for c in text.chars() {
        let c = match c {
            '\r' | '\n' | '\u{85}' | '\u{2028}' | '\u{2029}' => continue,
            '\t' => ' ',
            c if allowed(c) => c,
            _ => continue,
        };
        if out.len() + c.len_utf8() > room {
            break;
        }
        out.push(c);
    }
    out
}

/// `at`, or the character boundary before it.
fn floor_boundary(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Space,
    Word,
    Other,
}

fn class(c: char) -> Class {
    if c.is_whitespace() {
        Class::Space
    } else if c.is_alphanumeric() || c == '_' {
        Class::Word
    } else {
        Class::Other
    }
}

/// Ctrl+Right: past the run `at` is in, then past spaces; the next word's start, or the end.
fn word_after(text: &str, at: usize) -> usize {
    let mut chars = text[at..].char_indices();
    let Some((_, first)) = chars.next() else {
        return at;
    };
    let run = class(first);
    let mut spaces = run == Class::Space;
    for (k, c) in chars {
        let cl = class(c);
        if cl == Class::Space {
            spaces = true;
        } else if spaces || cl != run {
            return at + k;
        }
    }
    text.len()
}

/// Ctrl+Left: back past spaces, then past the run before them; that run's start.
fn word_before(text: &str, at: usize) -> usize {
    let mut chars = text[..at].char_indices().rev().peekable();
    let mut pos = at;
    while let Some(&(k, c)) = chars.peek() {
        if class(c) != Class::Space {
            break;
        }
        pos = k;
        chars.next();
    }
    let Some(&(_, c)) = chars.peek() else {
        return pos;
    };
    let run = class(c);
    for (k, c) in chars {
        if class(c) != run {
            break;
        }
        pos = k;
    }
    pos
}

impl<M> Ui<M> {
    /// Whether an enabled field has focus. The app's accelerators then let only Ctrl+K, F6, Tab
    /// and Esc through (TD-0009 §5.2), so typing never runs a command.
    #[must_use]
    pub fn field_focused(&self) -> bool {
        self.focused_field().is_some()
    }

    /// The focused field's selected text; empty when nothing is selected or no field has focus.
    #[must_use]
    pub fn selected_text(&self) -> &str {
        match self.focused_field() {
            Some(i) => {
                let f = &self.fields[self.nodes[i as usize].field as usize];
                let (a, b) = f.selection();
                self.nodes[i as usize].text.get(a..b).unwrap_or("")
            }
            None => "",
        }
    }

    /// The copy, cut or paste the last input asked for, which the platform carries out.
    pub fn take_clipboard(&mut self) -> Option<ClipboardRequest> {
        self.clipboard.take()
    }

    /// Windows' caret settings, at start and whenever they change.
    pub fn set_caret(&mut self, caret: Caret) {
        if self.caret != caret {
            self.caret = caret;
            self.reset_caret();
            self.damage_caret_field();
        }
    }

    /// Whether the window is active (has keyboard focus): only then is a caret or selection shown,
    /// or a tooltip. A window that goes inactive cancels its press, its hold and a scroll bar
    /// drag, whose release it may never see, and lets go of its tooltip; the pointer must move
    /// again to show one.
    pub fn set_active(&mut self, active: bool) {
        if self.active != active {
            self.active = active;
            self.reset_caret();
            self.damage_caret_field();
            if !active {
                self.cancel_press();
                self.scroll_release();
                self.tip_cancel(false);
                self.tip.over = NIL;
            }
        }
    }

    /// Runs the caret's blink at `now_ms` on the monotonic clock, as the platform does after every
    /// event and when its one-shot timer fires: the caret turns on or off if it's due, damaging
    /// its field. Returns how long until it's due again, or `None` when it no longer blinks (no
    /// field shows a caret, blinking is off, or it has blinked for [`Caret::BLINK_FOR_MS`]): the
    /// platform's timer then stops.
    pub fn caret_timer(&mut self, now_ms: u64) -> Option<u64> {
        let i = self.caret_field()?;
        let blink = u64::from(self.caret.blink_ms);
        let limit = u64::from(Caret::BLINK_FOR_MS);
        let epoch = *self.caret_epoch.get_or_insert(now_ms);
        let elapsed = now_ms.saturating_sub(epoch);
        let (on, next) = if blink == 0 || elapsed >= limit {
            (true, None)
        } else {
            let phase = elapsed / blink;
            let due = ((phase + 1) * blink).min(limit);
            (phase.is_multiple_of(2), Some(due - elapsed))
        };
        if on != self.caret_on {
            self.caret_on = on;
            self.damage_caret(i);
        }
        next
    }

    /// The caret, in logical pixels from the client area's top-left, while one shows (blinking
    /// off included): where an input method puts its windows.
    #[must_use]
    pub fn caret_rect(&self) -> Option<Rect> {
        let i = self.caret_field()?;
        let n = &self.nodes[i as usize];
        let f = &self.fields[n.field as usize];
        let line = f.line_box(n.rect.inset(n.style.padding));
        // Where it shows, in a scroll viewport too.
        let shift = self.shift_of(i);
        Some(Rect::new(
            line.x - f.scroll + f.x_of(f.caret as usize) + shift.x,
            line.y + shift.y,
            caret_logical(self.caret),
            line.h,
        ))
    }

    /// The pointer's shape where it is: an I-beam over a field, and while selecting in one.
    #[must_use]
    pub fn cursor(&self) -> Cursor {
        let i = if self.pressed == NIL {
            self.hover
        } else {
            self.pressed
        };
        match self.nodes.get(i as usize) {
            Some(n) if n.role == Role::Field && n.enabled => Cursor::Text,
            _ => Cursor::Arrow,
        }
    }

    /// The field `id`'s caret and selection.
    #[must_use]
    pub fn field_info(&self, id: NodeId) -> Option<FieldInfo> {
        let i = self.index(id)?;
        let f = self.fields.get(self.nodes[i as usize].field as usize)?;
        Some(FieldInfo {
            caret: f.caret,
            anchor: f.anchor,
            scroll: f.scroll,
        })
    }

    // ---- the field table ---------------------------------------------------------------------

    /// A field slot for node `node`, from the free list when possible.
    pub(crate) fn alloc_field(&mut self, node: u32) -> u32 {
        if let Some(f) = self.free_fields.pop() {
            let slot = &mut self.fields[f as usize];
            slot.node = node;
            slot.live = true;
            return f;
        }
        let f = u32::try_from(self.fields.len()).unwrap_or(NIL);
        self.fields.push(Field::new(node));
        f
    }

    /// Frees slot `f`, keeping its buffers for the next field.
    pub(crate) fn release_field(&mut self, f: u32) {
        let slot = &mut self.fields[f as usize];
        slot.live = false;
        slot.node = NIL;
        slot.name.clear();
        slot.placeholder.clear();
        slot.on_edit = None;
        slot.caret = 0;
        slot.anchor = 0;
        slot.scroll = 0.0;
        slot.stops.clear();
        slot.stops_valid = false;
        slot.width = 0.0;
        slot.view = 0.0;
        self.free_fields.push(f);
    }

    /// The view gave field `i` new text: the caret goes to its end, and layout finds its stops.
    pub(crate) fn field_text_set(&mut self, i: u32) {
        let len = u32::try_from(self.nodes[i as usize].text.len()).unwrap_or(u32::MAX);
        let f = &mut self.fields[self.nodes[i as usize].field as usize];
        f.caret = len;
        f.anchor = len;
        f.stops_valid = false;
        self.damage_node(i);
    }

    /// After layout: fields whose text changed get their caret stops, and every field scrolls to
    /// keep its caret in view in its content box, as wide as it now is.
    pub(crate) fn layout_fields(&mut self, text: &mut dyn TextSystem) {
        let caret_w = caret_logical(self.caret);
        for k in 0..self.fields.len() {
            let f = &mut self.fields[k];
            if !f.live {
                continue;
            }
            let i = f.node;
            let n = &self.nodes[i as usize];
            if !f.stops_valid {
                text.caret_stops(&n.text, &n.text_style, &mut f.stops);
                f.stops_valid = true;
                f.ordered = f.stops.windows(2).all(|w| w[0].x <= w[1].x);
                f.width = f.stops.iter().fold(0.0, |w, s| w.max(s.x));
                f.line = text.measure("", &n.text_style, f32::INFINITY).height;
            }
            f.view = n.rect.inset(n.style.padding).w.max(0.0);
            if f.fit_scroll(caret_w) {
                self.damage_node(i);
            }
        }
    }

    /// The focused node, if it's an enabled field.
    pub(crate) fn focused_field(&self) -> Option<u32> {
        let n = self.nodes.get(self.focus as usize)?;
        (n.role == Role::Field && n.enabled).then_some(self.focus)
    }

    /// The field that shows a caret: the focused one, while the window is active.
    fn caret_field(&self) -> Option<u32> {
        self.focused_field().filter(|_| self.active)
    }

    /// The caret was used: it shows, and blinks again from the next [`Ui::caret_timer`].
    pub(crate) fn reset_caret(&mut self) {
        self.caret_epoch = None;
        if !self.caret_on {
            self.caret_on = true;
            if let Some(i) = self.caret_field() {
                self.damage_caret(i);
            }
        }
    }

    /// Damages field `i`'s caret, where it is: all a blink repaints.
    fn damage_caret(&mut self, i: u32) {
        match self.caret_rect() {
            Some(r) => self.damage_rect(r),
            None => self.damage_node(i),
        }
    }

    fn damage_caret_field(&mut self) {
        if let Some(i) = self.focused_field() {
            self.damage_node(i);
        }
    }

    /// Puts field `i`'s caret at byte `to`, extending the selection or collapsing it, and keeps
    /// the caret in view.
    fn move_caret(&mut self, i: u32, to: usize, extend: bool) {
        let caret_w = caret_logical(self.caret);
        let f = &mut self.fields[self.nodes[i as usize].field as usize];
        let to = u32::try_from(to).unwrap_or(u32::MAX);
        let changed = f.caret != to || (!extend && f.anchor != to);
        f.caret = to;
        if !extend {
            f.anchor = to;
        }
        let scrolled = f.stops_valid && f.fit_scroll(caret_w);
        if changed || scrolled {
            self.damage_node(i);
        }
        self.reset_caret();
    }

    /// Selects all of field `i`, as Tab into it does.
    pub(crate) fn select_all(&mut self, i: u32) {
        let len = self.nodes[i as usize].text.len();
        self.move_caret(i, 0, false);
        self.move_caret(i, len, true);
    }

    /// A press at `p` in field `i` puts the caret there, or (`extend`, dragging) moves the
    /// selection's end there.
    pub(crate) fn field_pointer(&mut self, i: u32, p: Point, extend: bool) {
        let n = &self.nodes[i as usize];
        let f = &self.fields[n.field as usize];
        let content = n.rect.inset(n.style.padding);
        let shift = self.shift_of(i);
        let at = f.hit(&n.text, p.x - shift.x - content.x + f.scroll);
        self.move_caret(i, at, extend);
    }
}

impl<M: Clone> Ui<M> {
    /// Pastes `text` into the focused field, over its selection: what the platform does for
    /// [`ClipboardRequest::Paste`]. Returns whether a field took it.
    pub fn paste(&mut self, text: &str, out: &mut Vec<M>) -> bool {
        let Some(i) = self.editable_focus() else {
            return false;
        };
        self.replace_with(i, text, false, out)
    }

    /// Deletes the focused field's selection, once the platform has copied it for
    /// [`ClipboardRequest::Cut`]. Returns whether anything was deleted.
    pub fn cut_selection(&mut self, out: &mut Vec<M>) -> bool {
        let Some(i) = self.editable_focus() else {
            return false;
        };
        let (a, b) = self.fields[self.nodes[i as usize].field as usize].selection();
        self.replace(i, a, b, Insert::Nothing, out)
    }

    /// UI Automation's `SetValue`: the whole text replaced by `value`, as a paste would take it.
    pub(crate) fn field_set_value(&mut self, i: u32, value: &str, out: &mut Vec<M>) -> bool {
        if self.fields[self.nodes[i as usize].field as usize]
            .on_edit
            .is_none()
        {
            return false;
        }
        self.replace_with(i, value, true, out)
    }

    /// A typed character for the focused field. Returns whether a field has focus: it takes
    /// every character, inserting those it can hold.
    pub(crate) fn field_char(&mut self, c: char, out: &mut Vec<M>) -> bool {
        let Some(i) = self.focused_field() else {
            return false;
        };
        if allowed(c) && self.editable(i) {
            let (a, b) = self.fields[self.nodes[i as usize].field as usize].selection();
            self.replace(i, a, b, Insert::Char(c), out);
        }
        true
    }

    /// A key for the focused field `i`. `Some(used)` if the field decides; `None` leaves it to the
    /// rest of dispatch (Tab).
    pub(crate) fn field_key(
        &mut self,
        i: u32,
        key: crate::KeyEvent,
        out: &mut Vec<M>,
    ) -> Option<bool> {
        use crate::KeyCode;
        let mods = key.mods;
        if mods.alt || mods.win {
            return Some(false);
        }
        if !key.down {
            // The press decided; Space's release mustn't press the field like a button.
            return Some(matches!(
                key.key,
                KeyCode::Left
                    | KeyCode::Right
                    | KeyCode::Home
                    | KeyCode::End
                    | KeyCode::Backspace
                    | KeyCode::Delete
                    | KeyCode::Space
            ));
        }
        let node = &self.nodes[i as usize];
        let field = &self.fields[node.field as usize];
        let text = node.text.as_str();
        let caret = field.caret as usize;
        let (start, end) = field.selection();
        let selected = start != end;
        let letter = |l: u8| key.key == KeyCode::Other(u16::from(l)) && mods.ctrl && !mods.shift;
        match key.key {
            KeyCode::Left => {
                let to = if mods.ctrl {
                    field.stop_at_or_before(word_before(text, caret))
                } else if selected && !mods.shift {
                    start
                } else {
                    field.prev_stop(text, caret)
                };
                self.move_caret(i, to, mods.shift);
            }
            KeyCode::Right => {
                let to = if mods.ctrl {
                    field.stop_at_or_after(text, word_after(text, caret))
                } else if selected && !mods.shift {
                    end
                } else {
                    field.next_stop(text, caret)
                };
                self.move_caret(i, to, mods.shift);
            }
            KeyCode::Home => self.move_caret(i, 0, mods.shift),
            KeyCode::End => self.move_caret(i, text.len(), mods.shift),
            KeyCode::Backspace => {
                let (from, to) = if selected {
                    (start, end)
                } else if mods.ctrl {
                    (field.stop_at_or_before(word_before(text, caret)), caret)
                } else {
                    (field.prev_stop(text, caret), caret)
                };
                self.replace(i, from, to, Insert::Nothing, out);
            }
            KeyCode::Delete if mods.shift && !mods.ctrl => self.request(ClipboardRequest::Cut, i),
            KeyCode::Delete => {
                let (from, to) = if selected {
                    (start, end)
                } else if mods.ctrl {
                    (caret, field.stop_at_or_after(text, word_after(text, caret)))
                } else {
                    (caret, field.next_stop(text, caret))
                };
                self.replace(i, from, to, Insert::Nothing, out);
            }
            KeyCode::Insert if mods.ctrl && !mods.shift => {
                self.request(ClipboardRequest::Copy, i);
            }
            KeyCode::Insert if mods.shift && !mods.ctrl => {
                self.request(ClipboardRequest::Paste, i);
            }
            KeyCode::Space => {}
            KeyCode::Enter => {
                let msg = self.nodes[i as usize].msg.clone();
                return Some(msg.is_some_and(|msg| {
                    out.push(msg);
                    true
                }));
            }
            _ if letter(b'A') => self.select_all(i),
            _ if letter(b'C') => self.request(ClipboardRequest::Copy, i),
            _ if letter(b'X') => self.request(ClipboardRequest::Cut, i),
            _ if letter(b'V') => self.request(ClipboardRequest::Paste, i),
            KeyCode::Tab => return None,
            _ => return Some(false),
        }
        Some(true)
    }

    /// Asks the platform for a clipboard operation on field `i`, if it applies: a copy or cut
    /// needs a selection, a cut or paste an editable field.
    fn request(&mut self, r: ClipboardRequest, i: u32) {
        let (a, b) = self.fields[self.nodes[i as usize].field as usize].selection();
        let ok = match r {
            ClipboardRequest::Copy => a != b,
            ClipboardRequest::Cut => a != b && self.editable(i),
            ClipboardRequest::Paste => self.editable(i),
        };
        if ok {
            self.clipboard = Some(r);
        }
    }

    fn editable(&self, i: u32) -> bool {
        self.fields[self.nodes[i as usize].field as usize]
            .on_edit
            .is_some()
    }

    fn editable_focus(&self) -> Option<u32> {
        self.focused_field().filter(|&i| self.editable(i))
    }

    /// Replaces field `i`'s selection (`all`: its whole text) with `text` as a single-line field
    /// takes it.
    fn replace_with(&mut self, i: u32, text: &str, all: bool, out: &mut Vec<M>) -> bool {
        let n = &self.nodes[i as usize];
        let (a, b) = if all {
            (0, n.text.len())
        } else {
            self.fields[n.field as usize].selection()
        };
        let room = MAX_FIELD_LEN.saturating_sub(n.text.len() - (b - a));
        let clean = single_line(text, room);
        let insert = if clean.is_empty() {
            Insert::Nothing
        } else {
            Insert::Text(clean.into_boxed_str())
        };
        self.replace(i, a, b, insert, out)
    }

    /// Replaces bytes `start..end` of field `i` with `insert`, cut to the room left under
    /// [`MAX_FIELD_LEN`], and tells the view. Returns whether the text changed.
    fn replace(
        &mut self,
        i: u32,
        start: usize,
        end: usize,
        insert: Insert,
        out: &mut Vec<M>,
    ) -> bool {
        let f = self.nodes[i as usize].field as usize;
        let Some(on_edit) = self.fields[f].on_edit else {
            return false;
        };
        let n = &mut self.nodes[i as usize];
        let room = MAX_FIELD_LEN.saturating_sub(n.text.len() - (end - start));
        let insert = match insert {
            Insert::Text(t) if t.len() > room => {
                let cut = floor_boundary(&t, room);
                if cut == 0 {
                    Insert::Nothing
                } else {
                    Insert::Text(t[..cut].into())
                }
            }
            Insert::Char(c) if c.len_utf8() > room => Insert::Nothing,
            other => other,
        };
        if start == end && insert == Insert::Nothing {
            return false;
        }
        let mut buf = [0; 4];
        n.text.replace_range(start..end, insert.as_str(&mut buf));
        let caret = u32::try_from(start + insert.len()).unwrap_or(u32::MAX);
        let field = &mut self.fields[f];
        field.caret = caret;
        field.anchor = caret;
        field.stops_valid = false;
        self.damage_node(i);
        self.a11y_touch(i);
        self.reset_caret();
        out.push(on_edit(TextEdit {
            start: u32::try_from(start).unwrap_or(u32::MAX),
            end: u32::try_from(end).unwrap_or(u32::MAX),
            insert,
        }));
        true
    }
}

/// The caret's width in logical pixels.
pub(crate) fn caret_logical(caret: Caret) -> f32 {
    f32::from(u16::try_from(caret.width.max(1)).unwrap_or(u16::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Element, FakeText, Input, Insets, Key, KeyCode, KeyEvent, Mods, Motion, Size, Style, Ui,
    };

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Msg {
        Edit(TextEdit),
        Go,
    }

    /// A view-model with one field, its value applied from the field's edits.
    struct Model {
        value: String,
        editable: bool,
    }

    const FIELD: Key = Key::name("field");

    /// A 100-wide field (padding 4: content 92 wide, `FakeText`'s 13 px body advances 6.5 a
    /// character) and a button after it.
    fn frame(ui: &mut Ui<Msg>, m: &Model) {
        ui.build(|cx| {
            let field = Element::field(FIELD, &m.value, Msg::Edit)
                .name("Name")
                .placeholder("Type a name")
                .submit(Msg::Go)
                .style(Style {
                    width: crate::Dim::Fixed(100.0),
                    padding: Insets::all(4.0),
                    ..Style::default()
                });
            cx.add(if m.editable { field } else { field.read_only() });
            cx.add(Element::button(Key::name("after"), "After", Msg::Go));
        });
        ui.layout(&mut FakeText);
    }

    struct Setup {
        ui: Ui<Msg>,
        m: Model,
        out: Vec<Msg>,
    }

    impl Setup {
        fn new(value: &str) -> Self {
            let mut ui = Ui::new(Size { w: 200.0, h: 100.0 }, 1.0, Motion::default());
            ui.set_active(true);
            let m = Model {
                value: value.to_owned(),
                editable: true,
            };
            frame(&mut ui, &m);
            let mut s = Self {
                ui,
                m,
                out: Vec::new(),
            };
            let id = s.id();
            s.ui.set_focus(Some(id));
            s
        }

        fn id(&self) -> NodeId {
            self.ui.find(&[FIELD]).unwrap()
        }

        /// Delivers the messages to the model, then a frame.
        fn sync(&mut self) {
            for msg in self.out.drain(..) {
                if let Msg::Edit(e) = msg {
                    assert!(e.apply(&mut self.m.value), "{e:?}");
                }
            }
            frame(&mut self.ui, &self.m);
        }

        fn key(&mut self, key: KeyCode, mods: Mods) -> bool {
            let used = self.ui.dispatch(
                Input::Key(KeyEvent {
                    key,
                    down: true,
                    mods,
                }),
                &mut self.out,
            );
            self.sync();
            used
        }

        fn chars(&mut self, s: &str) {
            for c in s.chars() {
                assert!(self.ui.dispatch(Input::Char(c), &mut self.out));
                self.sync();
            }
        }

        fn value(&self) -> &str {
            self.ui.info(self.id()).unwrap().text
        }

        fn sel(&self) -> (u32, u32) {
            let f = self.ui.field_info(self.id()).unwrap();
            (f.anchor, f.caret)
        }
    }

    const NONE: Mods = Mods {
        shift: false,
        ctrl: false,
        alt: false,
        win: false,
    };
    const SHIFT: Mods = Mods {
        shift: true,
        ..NONE
    };
    const CTRL: Mods = Mods { ctrl: true, ..NONE };

    #[test]
    fn edits_apply_to_the_view_copy_and_check_their_range() {
        let mut s = String::from("héllo");
        let e = TextEdit {
            start: 1,
            end: 3,
            insert: Insert::Char('e'),
        };
        assert!(e.apply(&mut s));
        assert_eq!(s, "hello");
        // Inside a character, past the end, or backwards: not this text.
        for (start, end) in [(2, 2), (0, 9), (3, 1)] {
            let mut t = String::from("héllo");
            let bad = TextEdit {
                start,
                end,
                insert: Insert::Nothing,
            };
            assert!(!bad.apply(&mut t), "{start}..{end}");
            assert_eq!(t, "héllo");
        }
    }

    #[test]
    fn typing_inserts_at_the_caret_and_the_view_follows() {
        let mut s = Setup::new("");
        s.chars("cat");
        assert_eq!((s.value(), s.m.value.as_str()), ("cat", "cat"));
        assert_eq!(s.sel(), (3, 3));
        s.key(KeyCode::Left, NONE);
        s.chars("r");
        assert_eq!(s.value(), "cart");
        // Over a selection: Shift+Home selects "car", typing replaces it.
        s.key(KeyCode::Home, SHIFT);
        assert_eq!(s.sel(), (3, 0));
        assert_eq!(s.ui.selected_text(), "car");
        s.chars("ve");
        assert_eq!(s.value(), "vet");
        // Characters a field can't hold insert nothing, but are taken.
        for c in ['\u{1}', '\r', '\u{7f}', '\u{9b}', '\u{202e}', '\u{2067}'] {
            assert!(s.ui.dispatch(Input::Char(c), &mut s.out));
        }
        assert_eq!(s.out, [] as [Msg; 0]);
        assert_eq!(s.value(), "vet");
    }

    #[test]
    fn caret_keys_move_by_stop_and_word_and_collapse_selections() {
        let mut s = Setup::new("ab cd-ef");
        assert_eq!(s.sel(), (8, 8));
        s.key(KeyCode::Left, CTRL);
        assert_eq!(s.sel(), (6, 6), "the run 'ef'");
        s.key(KeyCode::Left, CTRL);
        assert_eq!(s.sel(), (5, 5), "the run '-'");
        s.key(KeyCode::Left, CTRL);
        assert_eq!(s.sel(), (3, 3));
        s.key(KeyCode::Left, CTRL);
        assert_eq!(s.sel(), (0, 0), "back past the space");
        s.key(KeyCode::Right, CTRL);
        assert_eq!(s.sel(), (3, 3), "past 'ab' and the space");
        s.key(
            KeyCode::Right,
            Mods {
                ctrl: true,
                shift: true,
                ..NONE
            },
        );
        assert_eq!(s.sel(), (3, 5));
        // Left and Right without Shift collapse to the side moved toward.
        s.key(KeyCode::Left, NONE);
        assert_eq!(s.sel(), (3, 3));
        s.key(KeyCode::End, SHIFT);
        s.key(KeyCode::Right, NONE);
        assert_eq!(s.sel(), (8, 8));
        s.key(KeyCode::Right, NONE);
        assert_eq!(s.sel(), (8, 8), "at the end already");
        assert_eq!(s.out, [] as [Msg; 0], "moving edits nothing");
    }

    #[test]
    fn deleting_removes_the_selection_a_stop_or_a_word() {
        let mut s = Setup::new("one two three");
        s.key(KeyCode::Backspace, CTRL);
        assert_eq!(s.value(), "one two ");
        s.key(KeyCode::Backspace, NONE);
        assert_eq!(s.value(), "one two");
        s.key(KeyCode::Home, NONE);
        s.key(KeyCode::Delete, CTRL);
        assert_eq!(s.value(), "two");
        s.key(KeyCode::Delete, NONE);
        assert_eq!(s.value(), "wo");
        s.key(KeyCode::Other(u16::from(b'A')), CTRL);
        s.key(KeyCode::Delete, NONE);
        assert_eq!((s.value(), s.m.value.as_str()), ("", ""));
        // Nothing to delete: no edit.
        s.key(KeyCode::Backspace, NONE);
        assert_eq!(s.out, [] as [Msg; 0]);
    }

    #[test]
    fn multi_byte_text_moves_and_deletes_whole_characters() {
        let mut s = Setup::new("a😀é");
        s.key(KeyCode::Left, NONE);
        assert_eq!(s.sel(), (5, 5), "before é (2 bytes)");
        s.key(KeyCode::Left, NONE);
        assert_eq!(s.sel(), (1, 1), "before the emoji (4 bytes)");
        s.key(KeyCode::Delete, NONE);
        assert_eq!(s.value(), "aé");
    }

    #[test]
    fn clipboard_keys_ask_the_platform() {
        let mut s = Setup::new("copy me");
        // Nothing selected: nothing to copy or cut.
        s.key(KeyCode::Other(u16::from(b'C')), CTRL);
        s.key(KeyCode::Other(u16::from(b'X')), CTRL);
        assert_eq!(s.ui.take_clipboard(), None);
        s.key(KeyCode::Home, SHIFT);
        for (key, mods, want) in [
            (
                KeyCode::Other(u16::from(b'C')),
                CTRL,
                ClipboardRequest::Copy,
            ),
            (KeyCode::Insert, CTRL, ClipboardRequest::Copy),
            (KeyCode::Other(u16::from(b'X')), CTRL, ClipboardRequest::Cut),
            (KeyCode::Delete, SHIFT, ClipboardRequest::Cut),
            (
                KeyCode::Other(u16::from(b'V')),
                CTRL,
                ClipboardRequest::Paste,
            ),
            (KeyCode::Insert, SHIFT, ClipboardRequest::Paste),
        ] {
            assert!(s.key(key, mods));
            assert_eq!(s.ui.take_clipboard(), Some(want), "{key:?} {mods:?}");
        }
        assert_eq!(s.ui.selected_text(), "copy me");
        assert!(s.ui.cut_selection(&mut s.out));
        s.sync();
        assert_eq!(s.m.value, "");
    }

    #[test]
    fn pastes_are_one_line_and_capped() {
        let mut s = Setup::new("");
        assert!(s.ui.paste("C:\\a\r\nb\tc\u{202e}d\u{7}", &mut s.out));
        s.sync();
        assert_eq!(s.m.value, "C:\\ab cd");
        // At the cap, a paste takes what fits, cut at a character boundary.
        let mut s = Setup::new(&"x".repeat(MAX_FIELD_LEN - 3));
        s.ui.paste("éé", &mut s.out);
        s.sync();
        assert_eq!(s.value().len(), MAX_FIELD_LEN - 1);
        assert!(s.value().ends_with('é'));
        s.chars("é");
        assert_eq!(s.value().len(), MAX_FIELD_LEN - 1, "no room for 2 bytes");
        s.chars("y");
        assert_eq!(s.value().len(), MAX_FIELD_LEN);
        // A paste of nothing a field holds changes nothing.
        let mut s = Setup::new("a");
        assert!(!s.ui.paste("\r\n", &mut s.out));
        assert_eq!(s.out, [] as [Msg; 0]);
    }

    #[test]
    fn enter_submits_and_other_keys_pass() {
        let mut s = Setup::new("x");
        assert!(s.key(KeyCode::Enter, NONE));
        // The submit message went to the model; nothing else did.
        for key in [
            KeyCode::Escape,
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::PageDown,
        ] {
            assert!(!s.key(key, NONE), "{key:?}");
        }
        assert!(!s.key(KeyCode::Left, Mods { alt: true, ..NONE }));
        // Space is a character: its key is taken and presses nothing.
        assert!(s.key(KeyCode::Space, NONE));
        assert!(s.ui.dispatch(
            Input::Key(KeyEvent {
                key: KeyCode::Space,
                down: false,
                mods: NONE
            }),
            &mut s.out
        ));
        assert_eq!(s.out, [] as [Msg; 0]);
        // Tab moves on, to the button.
        assert!(s.key(KeyCode::Tab, NONE));
        assert!(!s.ui.field_focused());
    }

    #[test]
    fn read_only_fields_select_and_copy_but_never_change() {
        let mut s = Setup::new("fixed");
        s.m.editable = false;
        frame(&mut s.ui, &s.m);
        s.chars("x");
        s.key(KeyCode::Backspace, NONE);
        assert!(!s.ui.paste("y", &mut s.out));
        assert_eq!(s.value(), "fixed");
        s.key(KeyCode::Other(u16::from(b'A')), CTRL);
        s.key(KeyCode::Other(u16::from(b'X')), CTRL);
        assert_eq!(s.ui.take_clipboard(), None, "no cut");
        s.key(KeyCode::Other(u16::from(b'C')), CTRL);
        assert_eq!(s.ui.take_clipboard(), Some(ClipboardRequest::Copy));
        assert_eq!(s.out, [] as [Msg; 0]);
    }

    #[test]
    fn the_view_wins_when_it_keeps_another_value() {
        let mut s = Setup::new("ab");
        // The view rejects the edit: its value stays, and the caret goes to its end.
        s.ui.dispatch(Input::Char('c'), &mut s.out);
        s.out.clear();
        frame(&mut s.ui, &s.m);
        assert_eq!((s.value(), s.sel()), ("ab", (2, 2)));
        // The view sets a value of its own.
        s.m.value = "longer".into();
        frame(&mut s.ui, &s.m);
        assert_eq!((s.value(), s.sel()), ("longer", (6, 6)));
    }

    #[test]
    fn clicks_place_the_caret_and_drags_select() {
        let mut s = Setup::new("abcdef");
        let r = s.ui.info(s.id()).unwrap().rect;
        // Content starts at x 4; characters are 6.5 wide: 2 characters in is x 17.
        let at = |x: f32| crate::Point {
            x,
            y: r.y + r.h / 2.0,
        };
        s.ui.dispatch(Input::PointerDown(at(17.5)), &mut s.out);
        assert_eq!(s.sel(), (2, 2));
        assert_eq!(s.ui.cursor(), Cursor::Text);
        s.ui.dispatch(Input::PointerMove(at(36.0)), &mut s.out);
        assert_eq!(s.sel(), (2, 5));
        s.ui.dispatch(Input::PointerUp(at(36.0)), &mut s.out);
        assert_eq!(s.out, [] as [Msg; 0], "a click on a field submits nothing");
        assert_eq!(s.ui.selected_text(), "cde");
    }

    #[test]
    fn long_text_scrolls_to_keep_the_caret_in_view() {
        // 30 characters: 195 px of text in a 92 px box.
        let mut s = Setup::new(&"w".repeat(30));
        let scroll = |s: &Setup| s.ui.field_info(s.id()).unwrap().scroll;
        assert_eq!(
            scroll(&s),
            195.0 + 1.0 - 92.0,
            "the caret at the end, in view"
        );
        s.key(KeyCode::Home, NONE);
        assert_eq!(scroll(&s), 0.0);
        s.key(KeyCode::End, NONE);
        assert_eq!(scroll(&s), 104.0);
        // Deleting makes the text fit again: no scroll.
        s.key(KeyCode::Other(u16::from(b'A')), CTRL);
        s.chars("w");
        assert_eq!(scroll(&s), 0.0);
    }

    #[test]
    fn tab_in_selects_all() {
        let mut s = Setup::new("all of it");
        s.ui.set_focus(None);
        s.key(KeyCode::Tab, NONE);
        assert!(s.ui.field_focused());
        assert_eq!(s.ui.selected_text(), "all of it");
    }

    #[test]
    fn the_caret_blinks_then_rests_and_only_where_it_shows() {
        let mut s = Setup::new("x");
        let blink = u64::from(Caret::WINDOWS.blink_ms);
        assert_eq!(s.ui.caret_timer(1_000), Some(blink));
        assert_eq!(s.ui.caret_timer(1_000 + blink), Some(blink));
        assert!(!s.ui.caret_on);
        // A keystroke shows it again and starts over.
        s.key(KeyCode::Left, NONE);
        assert!(s.ui.caret_on);
        assert_eq!(s.ui.caret_timer(2_000), Some(blink));
        // After BLINK_FOR_MS it rests, on, and the timer stops.
        let end = 2_000 + u64::from(Caret::BLINK_FOR_MS);
        assert_eq!(s.ui.caret_timer(end - 1), Some(1));
        assert_eq!(s.ui.caret_timer(end), None);
        assert!(s.ui.caret_on);
        // No blinking setting: no timer at all.
        s.ui.set_caret(Caret {
            width: 2,
            blink_ms: 0,
        });
        assert_eq!(s.ui.caret_timer(9_000), None);
        // An inactive window, or no field with focus: no caret, no timer.
        s.ui.set_caret(Caret::WINDOWS);
        s.ui.set_active(false);
        assert_eq!((s.ui.caret_timer(10_000), s.ui.caret_rect()), (None, None));
        s.ui.set_active(true);
        assert!(s.ui.caret_rect().is_some());
        s.ui.set_focus(None);
        assert_eq!(s.ui.caret_timer(10_000), None);
    }

    #[test]
    fn assistive_technology_sees_a_text_input_and_can_set_its_value() {
        use accesskit::{Action, ActionData, ActionRequest, Role as AkRole, TreeId};
        let mut s = Setup::new("");
        let tree = s.ui.a11y_tree();
        let (id, node) = tree
            .nodes
            .iter()
            .find(|(_, n)| n.role() == AkRole::TextInput)
            .unwrap();
        assert_eq!(node.label(), Some("Name"));
        assert_eq!(node.value(), Some(""));
        assert_eq!(node.placeholder(), Some("Type a name"));
        assert!(!node.is_read_only());
        assert!(node.supports_action(Action::SetValue) && node.supports_action(Action::Focus));
        assert!(
            !node.supports_action(Action::Click),
            "a field isn't pressed"
        );
        let request = |action, data| ActionRequest {
            action,
            target_tree: TreeId::ROOT,
            target_node: *id,
            data,
        };
        // A value is taken as a paste would take it.
        let set = Some(ActionData::Value("a\r\nb".into()));
        assert!(s.ui.a11y_action(&request(Action::SetValue, set), &mut s.out));
        s.sync();
        assert_eq!(s.m.value, "ab");
        assert!(!s.ui.a11y_action(&request(Action::Click, None), &mut s.out));
        assert_eq!(s.out, [] as [Msg; 0]);
        // Read-only: no SetValue.
        s.m.editable = false;
        frame(&mut s.ui, &s.m);
        let update = s.ui.a11y_update().unwrap();
        let node = &update.nodes.iter().find(|(i, _)| i == id).unwrap().1;
        assert!(node.is_read_only() && !node.supports_action(Action::SetValue));
        assert_eq!(node.value(), Some("ab"));
        let set = Some(ActionData::Value("c".into()));
        assert!(
            !s.ui
                .a11y_action(&request(Action::SetValue, set), &mut s.out)
        );
        assert_eq!(s.value(), "ab");
    }

    #[test]
    fn word_boundaries() {
        let t = "C:\\Program Files\\App  x";
        assert_eq!(word_after(t, 0), 1);
        assert_eq!(word_after(t, 1), 3);
        assert_eq!(word_after(t, 3), 11);
        assert_eq!(word_after(t, 16), 17);
        assert_eq!(word_after(t, 17), 22);
        assert_eq!(word_after(t, t.len()), t.len());
        assert_eq!(word_before(t, t.len()), 22);
        assert_eq!(word_before(t, 22), 17);
        assert_eq!(word_before(t, 3), 1);
        assert_eq!(word_before(t, 0), 0);
    }
}
