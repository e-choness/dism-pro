//! Ctrl+C / Ctrl+Break → cancellation (TD-0001 §5.3). The first press cancels the
//! [`CancelToken`] (jobs stop at the next change boundary) and sets the cancel event that
//! cancellable DISM calls wait on. A second press falls through to the default handler, which
//! ends the process; the journal keeps whatever already ran undoable.

use std::sync::{
    OnceLock,
    atomic::{AtomicBool, Ordering},
};

use dpro_core::cancel::CancelToken;
use windows_sys::{
    Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT, SetConsoleCtrlHandler},
    core::BOOL,
};

use crate::{
    error::{Result, last_error_if_false},
    event::Event,
};

struct Handler {
    token: CancelToken,
    event: Event,
    pressed: AtomicBool,
}

static HANDLER: OnceLock<Handler> = OnceLock::new();

/// Installs the process-wide handler for `token` and returns the cancel event it sets.
/// Later calls return the first event and keep the first token.
///
/// # Errors
/// OS errors from `CreateEventW` or `SetConsoleCtrlHandler`.
pub fn cancel_on_ctrl_c(token: &CancelToken) -> Result<&'static Event> {
    if let Some(h) = HANDLER.get() {
        return Ok(&h.event);
    }
    let event = Event::manual_reset()?;
    let mut first = false;
    let h = HANDLER.get_or_init(|| {
        first = true;
        Handler {
            token: token.clone(),
            event,
            pressed: AtomicBool::new(false),
        }
    });
    if first {
        // SAFETY: `on_ctrl` is a valid `PHANDLER_ROUTINE` for the life of the process.
        let ok = unsafe { SetConsoleCtrlHandler(Some(on_ctrl), 1) };
        last_error_if_false(ok, "SetConsoleCtrlHandler")?;
    }
    Ok(&h.event)
}

/// Runs on a thread the system creates for the signal.
unsafe extern "system" fn on_ctrl(ctrl_type: u32) -> BOOL {
    if ctrl_type != CTRL_C_EVENT && ctrl_type != CTRL_BREAK_EVENT {
        return 0;
    }
    let Some(h) = HANDLER.get() else {
        return 0;
    };
    if h.pressed.swap(true, Ordering::AcqRel) {
        return 0; // second press: let the default handler end the process
    }
    h.token.cancel();
    // ignore: if the event can't be set, the token still stops the job between changes.
    let _ = h.event.set();
    1
}
