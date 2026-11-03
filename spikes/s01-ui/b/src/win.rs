//! The window mode on Windows (winit + AccessKit): the G1 startup, UI Automation and glass gates.

use std::{error::Error, rc::Rc, time::Duration};

use scene::{
    Screen, State,
    data::{self, Rows},
    tokens::Theme,
};
use slint::ComponentHandle;
use windows_sys::Win32::{
    Foundation::HWND,
    Graphics::Dwm::{
        DWMWA_SYSTEMBACKDROP_TYPE, DwmExtendFrameIntoClientArea, DwmSetWindowAttribute,
    },
    UI::Controls::MARGINS,
};

use crate::{App, model};

pub fn run(args: &[String]) -> Result<(), Box<dyn Error>> {
    probe::arm(args);
    probe::hook_first_paint();
    let screen = probe::value(args, "--screen")
        .and_then(|s| Screen::parse(&s))
        .unwrap_or(Screen::List);
    let theme = probe::value(args, "--theme")
        .and_then(|s| Theme::parse(&s))
        .unwrap_or(Theme::Light);
    let glass = probe::flag(args, "--glass");
    let (w, h) = probe::size(args, (1280, 800));

    let app = App::new()?;
    model::fill(&app, Rc::new(Rows::generate(data::ROWS)));
    model::apply(&app, &State::snapshot(screen, theme), glass);
    app.window()
        .set_size(slint::LogicalSize::new(w as f32, h as f32));

    let weak = app.as_weak();
    app.on_nav_clicked(move |i| {
        if let Some(app) = weak.upgrade() {
            let screen = i32::from(i as usize == data::NAV_APPS);
            app.set_screen(screen);
        }
    });
    let weak = app.as_weak();
    app.on_row_clicked(move |_| {
        if let Some(app) = weak.upgrade() {
            app.set_screen(2);
        }
    });
    let weak = app.as_weak();
    app.on_drawer_closed(move || {
        if let Some(app) = weak.upgrade() {
            app.set_screen(1);
        }
    });
    if glass {
        // The HWND exists once the event loop has created the window.
        let weak = app.as_weak();
        slint::Timer::single_shot(Duration::ZERO, move || {
            if let Some(app) = weak.upgrade() {
                mica(&app);
            }
        });
    }
    app.run()?;
    Ok(())
}

/// Mica behind the whole client area (ADR-0007), as far as Slint's presentation lets it through.
fn mica(app: &App) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = app.window().window_handle();
    let Ok(handle) = handle.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(h) = handle.as_raw() else {
        return;
    };
    let hwnd = h.hwnd.get() as HWND;
    let backdrop: i32 = 2; // DWMSBT_MAINWINDOW
    let margins = MARGINS {
        cxLeftWidth: -1,
        cxRightWidth: -1,
        cyTopHeight: -1,
        cyBottomHeight: -1,
    };
    // SAFETY: hwnd is this app's live top-level window; the pointers are to live locals of the
    // sizes passed.
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE as u32,
            (&raw const backdrop).cast(),
            4,
        );
        DwmExtendFrameIntoClientArea(hwnd, &raw const margins);
    }
}
