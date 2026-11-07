//! UI toolkit, widgets, theming, accessibility/UIA (ADR-0002, TD-0009). Fluent IDs and design
//! tokens only: no string literals, colors or sizes.
//!
//! Layer: presentation.
//! Allowed workspace dependencies: `dpro-core`, `dpro-app`, `dpro-win`.
//!
//! The toolkit core is a retained, keyed element tree that views rebuild cheaply each frame, with
//! flexbox-subset layout, pointer and keyboard input, focus, damage tracking and an animation
//! clock. It paints on a software [`Canvas`] in [`Theme`] colors and describes itself to assistive
//! technology as an [`accesskit`] tree. It is platform-free, has no `unsafe`, and runs in T1 against
//! [`FakeText`]. On Windows, `platform` hosts it in a `dpro_win::gui` window (TD-0009 §5.1).
//!
//! A frame, on the UI thread:
//! 1. [`Ui::dispatch`] turns input into the view's messages, and the app updates its view-model.
//! 2. [`Ui::build`] reconciles the view's description of the page with the retained tree.
//! 3. [`Ui::layout`] lays out what changed and records the damage.
//! 4. [`Ui::paint`] repaints [`Ui::damage`] into the canvas; the presenter shows it and calls
//!    [`Ui::clear_damage`].
//! 5. While a screen reader or other client listens, [`Ui::a11y_update`] gives what it must learn.
//!
//! ```
//! use dpro_ui::{Canvas, Element, FakeText, Input, Key, Motion, Size, Theme, Ui};
//!
//! #[derive(Clone, Debug, PartialEq)]
//! enum Msg {
//!     Save,
//! }
//!
//! let mut ui = Ui::new(Size { w: 400.0, h: 300.0 }, 1.0, Motion::default());
//! ui.build(|cx| {
//!     cx.add(Element::button(Key::name("save"), "Save", Msg::Save));
//! });
//! ui.layout(&mut FakeText);
//!
//! // Paint the frame, in the light theme, into window-sized pixels; a presenter would show the
//! // damage, then clear it.
//! let (w, h) = ui.pixel_size();
//! let mut pixels = vec![0; (w * h) as usize];
//! let mut canvas = Canvas::new(&mut pixels, w, h).unwrap();
//! ui.paint(&mut canvas, &mut FakeText, &Theme::LIGHT);
//! ui.clear_damage();
//!
//! let save = ui.find(&[Key::name("save")]).and_then(|id| ui.info(id)).unwrap();
//! let at = save.rect.center();
//! let mut msgs = Vec::new();
//! ui.dispatch(Input::PointerDown(at), &mut msgs);
//! ui.dispatch(Input::PointerUp(at), &mut msgs);
//! assert_eq!(msgs, [Msg::Save]);
//! ```

#![forbid(unsafe_code)]

mod a11y;
mod build;
mod canvas;
mod damage;
mod field;
mod geom;
mod input;
mod layout;
pub mod materials;
mod paint;
#[cfg(windows)]
pub mod platform;
mod pool;
mod popup;
mod scroll;
mod style;
mod table;
mod text;
mod theme;
mod tooltip;
mod tree;
mod ui;

pub use accesskit;
pub use build::Cx;
pub use canvas::Canvas;
pub use dpro_app::table::{ColumnId, Order, Pick, RowKey, Tri};
pub use field::{Caret, ClipboardRequest, Cursor, FieldInfo, Insert, MAX_FIELD_LEN, TextEdit};
pub use geom::{Insets, Point, Rect, RectPx, Size};
pub use input::{Input, KeyCode, KeyEvent, Mods};
pub use popup::{Placement, Popup, PopupKind, place};
pub use scroll::{Axes, ScrollBar, ScrollInfo};
pub use style::{Align, Axis, Dim, Justify, Style};
pub use table::{
    Cells, Column, Row, Status, TABLE_BODY, TABLE_CHECK, TABLE_HEADER, Table, TableEvent, Track,
};
pub use text::{CaretStop, FakeText, GlyphSink, Icon, TextMetrics, TextStyle, TextSystem};
pub use theme::{Color, Scheme, SysColors, Theme, Tone, tokens};
pub use tree::{Element, Group, Key, Look, NodeId, NodeInfo, Radius, Role};
pub use ui::{MAX_TEXT_SCALE, Motion, Ui};
