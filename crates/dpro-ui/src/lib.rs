//! UI toolkit, widgets, theming, accessibility/UIA (ADR-0002, TD-0009). Fluent IDs and design
//! tokens only: no string literals, colors or sizes.
//!
//! Layer: presentation.
//! Allowed workspace dependencies: `dpro-core`, `dpro-app`, `dpro-win`.
//!
//! The toolkit core is a retained, keyed element tree that views rebuild cheaply each frame, with
//! flexbox-subset layout, pointer and keyboard input, focus, damage tracking and an animation
//! clock. It is platform-free, has no `unsafe`, and runs in T1 against [`FakeText`]. Painting, the
//! accessibility tree and the Win32 platform build on it in later slices (TD-0009 §11).
//!
//! A frame, on the UI thread:
//! 1. [`Ui::dispatch`] turns input into the view's messages, and the app updates its view-model.
//! 2. [`Ui::build`] reconciles the view's description of the page with the retained tree.
//! 3. [`Ui::layout`] lays out what changed and records the damage.
//! 4. The presenter repaints [`Ui::damage`] and calls [`Ui::clear_damage`].
//!
//! ```
//! use dpro_ui::{Element, FakeText, Input, Key, Motion, Size, Ui};
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
//! let save = ui.find(&[Key::name("save")]).and_then(|id| ui.info(id)).unwrap();
//! let at = save.rect.center();
//! let mut msgs = Vec::new();
//! ui.dispatch(Input::PointerDown(at), &mut msgs);
//! ui.dispatch(Input::PointerUp(at), &mut msgs);
//! assert_eq!(msgs, [Msg::Save]);
//! ```

#![forbid(unsafe_code)]

mod build;
mod damage;
mod geom;
mod input;
mod layout;
mod pool;
mod style;
mod text;
mod tree;
mod ui;

pub use build::Cx;
pub use dpro_app::table::Tri;
pub use geom::{Insets, Point, Rect, RectPx, Size};
pub use input::{Input, KeyCode, KeyEvent, Mods};
pub use style::{Align, Axis, Dim, Justify, Style};
pub use text::{FakeText, TextMeasure, TextMetrics, TextStyle};
pub use tree::{Element, Key, NodeId, NodeInfo, Role};
pub use ui::{Motion, Ui};
