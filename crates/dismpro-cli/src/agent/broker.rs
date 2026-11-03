//! `dismpro-cli --agent-broker --user-sid <SID>`: the elevated half of agent access (TD-0013
//! §5.1, §15). A front starts it through one UAC prompt. It serves that user's fronts in this
//! session over the broker pipe (up to `pipe::MAX_INSTANCES` at once), each on its own thread,
//! and exits 15 minutes after the last one leaves.

use std::{
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    thread,
    time::{Duration, Instant},
};

use dpro_broker::peer;
use dpro_cli_fmt::exit::ExitCode;
use dpro_win::{
    pipe::{self, Pipe},
    privilege,
};

use super::tools::BrokerTools;
use crate::error::CliError;

/// How long the broker waits with no front connected before it exits (TD-0013 §15).
const IDLE_EXIT: Duration = Duration::from_mins(15);
/// How often the watchdog looks.
const WATCH_EVERY: Duration = Duration::from_secs(30);
/// How long to wait for a free instance when every one serves a front.
const BUSY_RETRY: Duration = Duration::from_millis(200);
/// `HRESULT_FROM_WIN32(ERROR_ACCESS_DENIED)`: the pipe name is taken (first-instance flag).
const NAME_TAKEN: u32 = 0x8007_0005;
/// `HRESULT_FROM_WIN32(ERROR_PIPE_BUSY)`: every instance is in use.
const PIPE_BUSY: u32 = 0x8007_00E7;

/// Serves fronts until the broker has been idle for [`IDLE_EXIT`], then exits the process.
/// Returns at once with [`ExitCode::Ok`] when another broker already serves this user's
/// session: the front that started this one connects to that one.
///
/// # Errors
/// Not elevated; the pipe or a thread can't be created.
pub(crate) fn run(user_sid: &str) -> Result<ExitCode, CliError> {
    if !privilege::is_elevated()? {
        return Err(CliError::NeedsElevation);
    }
    let name = peer::pipe_name(user_sid, pipe::current_session_id()?);
    let mut listening = match Pipe::create(&name, user_sid) {
        Ok(p) => p,
        // Another broker has it, or something squats the name, which the front's check of
        // the server catches.
        Err(e) if is(&e, NAME_TAKEN) || is(&e, PIPE_BUSY) => return Ok(ExitCode::Ok),
        Err(e) => return Err(e.into()),
    };
    let tools = Arc::new(BrokerTools::start()?);
    let activity = Arc::new(Activity::new());
    {
        let (activity, tools) = (Arc::clone(&activity), Arc::clone(&tools));
        thread::Builder::new()
            .name("idle-watch".into())
            .spawn(move || watch(&activity, &tools))?;
    }
    let broker_pid = std::process::id();
    loop {
        let connected = listening.accept();
        // A new instance before serving this one, so the next front can connect meanwhile.
        let next = next_instance(&listening)?;
        let conn = std::mem::replace(&mut listening, next);
        if let Err(e) = connected {
            eprintln!("dismpro-cli broker: a connection failed: {e}");
            continue;
        }
        let Some(guard) = Activity::enter(&activity) else {
            continue; // shutting down: dropping the connection tells the front to start over
        };
        let (tools, sid) = (Arc::clone(&tools), user_sid.to_owned());
        thread::Builder::new().name("front".into()).spawn(move || {
            let _guard = guard;
            serve_front(&conn, &sid, &tools, broker_pid);
        })?;
    }
}

/// Checks who connected, then serves its calls until it disconnects.
fn serve_front(conn: &Pipe, user_sid: &str, tools: &BrokerTools, broker_pid: u32) {
    match peer::check_front(conn, user_sid) {
        Ok(_) => {
            if let Err(e) = dpro_broker::serve(conn, tools, broker_pid) {
                eprintln!("dismpro-cli broker: {e}");
            }
        }
        Err(e) => eprintln!("dismpro-cli broker: refused a client: {e}"),
    }
}

/// Another listening instance, waiting while all of them serve fronts.
fn next_instance(pipe: &Pipe) -> Result<Pipe, CliError> {
    loop {
        match pipe.next_instance() {
            Err(e) if is(&e, PIPE_BUSY) => thread::sleep(BUSY_RETRY),
            other => return Ok(other?),
        }
    }
}

fn is(e: &dpro_win::Error, hresult: u32) -> bool {
    matches!(e, dpro_win::Error::Os(os) if os.hresult_bits() == hresult)
}

/// Connected fronts and how long there have been none.
#[derive(Debug)]
struct Activity(Mutex<Idle>);

#[derive(Debug)]
struct Idle {
    fronts: usize,
    since: Instant,
    /// Set once the watchdog decides to exit; no front is accepted after.
    closing: bool,
}

/// One connected front; dropping it marks the front gone.
#[derive(Debug)]
struct Guard(Arc<Activity>);

impl Activity {
    fn new() -> Self {
        Self(Mutex::new(Idle {
            fronts: 0,
            since: Instant::now(),
            closing: false,
        }))
    }

    fn lock(&self) -> MutexGuard<'_, Idle> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn enter(this: &Arc<Self>) -> Option<Guard> {
        let mut idle = this.lock();
        if idle.closing {
            return None;
        }
        idle.fronts += 1;
        Some(Guard(Arc::clone(this)))
    }

    /// Whether the broker should exit now; if so, no front gets in after this.
    fn should_exit(&self, after: Duration) -> bool {
        let mut idle = self.lock();
        if idle.fronts == 0 && idle.since.elapsed() >= after {
            idle.closing = true;
        }
        idle.closing
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let mut idle = self.0.lock();
        idle.fronts = idle.fronts.saturating_sub(1);
        if idle.fronts == 0 {
            idle.since = Instant::now();
        }
    }
}

/// Exits the process once no front has been connected for [`IDLE_EXIT`]. DISM shuts down
/// first, so it doesn't leave a session or a mounted hive behind.
fn watch(activity: &Activity, tools: &BrokerTools) {
    while !activity.should_exit(IDLE_EXIT) {
        thread::sleep(WATCH_EVERY);
    }
    tools.shutdown();
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_idle_clock_starts_when_the_last_front_leaves() {
        let a = Arc::new(Activity::new());
        let one = Activity::enter(&a).unwrap();
        let two = Activity::enter(&a).unwrap();
        assert!(!a.should_exit(Duration::ZERO));
        drop(one);
        assert!(!a.should_exit(Duration::ZERO), "a front is still there");
        drop(two);
        assert!(!a.should_exit(Duration::from_secs(3600)));
        assert!(a.should_exit(Duration::ZERO));
        assert!(Activity::enter(&a).is_none(), "closing lets no one in");
    }
}
