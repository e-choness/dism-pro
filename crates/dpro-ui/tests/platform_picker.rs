//! The path picker end to end on the platform (TD-0009 A3.5 (12), A3.8's failure table, slice
//! 6e), T2 only: `fixtures/picker.rs`'s page hosted by `platform::Host` in a real dpro-win window
//! that has no system dialog (`Host::without_file_dialog`: the path a `WinPE` without the dialog
//! takes). Browse… asks for a pick in an event's update; the host hands it to the window once the
//! view is built; the window answers once the event is done, `Unavailable`; the answer comes back
//! as the page's message, which opens the in-app picker. Closed and asked again, the window
//! answers from its memory of the session at once, with the same code, and the picker opens
//! again.
//!
//! The steps run in the window's first events (the presenter, Windows' settings and the DPI that
//! a window sends while it's created), each one taken when the model's clock runs, so no input or
//! message loop is needed.

#![cfg(windows)]
#![allow(missing_docs)]

#[path = "../benches/fixtures/picker.rs"]
mod picker;

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use dpro_core::OsError;
use dpro_ui::{
    Cx, FakeText,
    materials::GlassPref,
    platform::{
        Host, Model,
        file_dialog::{Pick, PickError, Picked},
    },
};
use dpro_win::gui::{Compose, Window, WindowOptions};
use picker::Msg;

thread_local! {
    /// What the window answered, in order: the platform's answers, before the page's message.
    static ANSWERS: RefCell<Vec<Picked>> = const { RefCell::new(Vec::new()) };
}

/// The window's answer, kept, then as the page's message.
fn kept(picked: Picked) -> Msg {
    ANSWERS.with(|a| a.borrow_mut().push(picked.clone()));
    picker::answer(picked)
}

/// What happened, in order: `page` or `sheet` for each build (once while it stays the same),
/// `asked` for a pick handed to the host, and the answers.
type Log = Rc<RefCell<Vec<&'static str>>>;

/// The page, with a script: Browse…; then, once the in-app picker has been built, Close and
/// Browse… again.
struct Page {
    model: picker::Model,
    log: Log,
    step: u8,
    /// The in-app picker was built since the last step.
    sheet_built: Cell<bool>,
}

impl Page {
    fn note(&self, what: &'static str) {
        let mut log = self.log.borrow_mut();
        if log.last() != Some(&what) {
            log.push(what);
        }
    }
}

impl Model for Page {
    type Msg = Msg;

    fn view(&self, cx: &mut Cx<'_, Msg>) {
        picker::view(&self.model, cx);
        if self.model.open {
            self.sheet_built.set(true);
            self.note("sheet");
        } else {
            self.note("page");
        }
    }

    fn update(&mut self, msg: Msg) {
        if let Msg::Picked(answer) = &msg {
            self.note(match answer {
                picker::Answer::Unavailable => "unavailable",
                picker::Answer::Failed => "failed",
                picker::Answer::Cancelled => "cancelled",
                picker::Answer::Path(_) => "a path",
            });
        }
        self.model.update(msg);
    }

    fn wake(&mut self, _now_ms: u64) -> Option<u64> {
        match self.step {
            0 => {
                self.model.update(Msg::Browse);
                self.step = 1;
            }
            1 if self.sheet_built.take() => {
                self.model.update(Msg::Close);
                self.model.update(Msg::Browse);
                self.step = 2;
            }
            _ => {}
        }
        None
    }

    fn picks(&mut self, mut f: impl FnMut(&Pick<'_>, fn(Picked) -> Msg)) {
        let log = Rc::clone(&self.log);
        self.model.picks(|pick, _| {
            log.borrow_mut().push("asked");
            f(pick, kept);
        });
    }
}

#[test]
fn without_the_dialog_browse_opens_the_in_app_picker_every_time() {
    let log = Log::default();
    let page = Page {
        model: picker::Model::with_entries(40),
        log: Rc::clone(&log),
        step: 0,
        sheet_built: Cell::new(false),
    };
    let host = Host::new(page, Box::new(FakeText), GlassPref::Auto).without_file_dialog();
    let options = WindowOptions {
        title: "dpro-ui platform test: picker",
        size: (640, 480),
        min_size: (320, 240),
        compose: Compose::Never,
    };
    let window = Window::new(&options, Box::new(host)).expect("a window");
    assert_eq!(
        *log.borrow(),
        [
            "page",
            "asked",
            "unavailable",
            "sheet",
            "page",
            "asked",
            "unavailable",
            "sheet"
        ],
        "each Browse… asked once, after its event, and the answer opened the in-app picker"
    );
    // The window's own answer, without COM, the second time from its memory.
    let refused = OsError::from_win32(50, "WindowCx::without_file_dialog");
    let answers = ANSWERS.with(|a| a.borrow().clone());
    assert_eq!(
        answers,
        [
            Err(PickError::Unavailable(refused)),
            Err(PickError::Unavailable(refused))
        ]
    );
    drop(window);
}
