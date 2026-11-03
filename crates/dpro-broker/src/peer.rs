//! Who may be on the other end of the broker pipe (TD-0013 §8.1, §15). Every answer comes from
//! the kernel (the pipe's peer pid, the process's image and token), never from the peer's own
//! messages.
//!
//! - The broker accepts a front that runs this same executable, as the user the pipe was
//!   created for.
//! - A front accepts a broker that runs this executable, or the GUI next to it (an elevated GUI
//!   hosts the broker in-process).
//!
//! "Same executable" means the same file by canonical path and the same SHA-256 when the peer
//! runs the file this process runs. Releases are unsigned (owner decision #17), so there's no
//! Authenticode to check instead.

use std::{
    fs,
    path::{Path, PathBuf},
};

use dpro_win::pipe::{self, Pipe};

use crate::Error;

/// The GUI, which hosts the broker when it runs elevated (TD-0013 §5.1).
const GUI: &str = "dismpro.exe";

/// The pipe name for one user's broker in one Remote Desktop session. The front works it out
/// and passes it to the broker it starts, since an elevation with other credentials gives the
/// broker another user and logon session.
#[must_use]
pub fn pipe_name(user_sid: &str, session: u32) -> String {
    format!("dismpro-agent-{user_sid}-{session}")
}

/// The broker's check of a connected front. Returns the front's pid.
///
/// # Errors
/// [`Error::Untrusted`] when the front runs another executable or as another user; OS failures.
pub fn check_front(pipe: &Pipe, user_sid: &str) -> Result<u32, Error> {
    let pid = pipe.peer_pid().map_err(|e| os(&e))?;
    let own = own_exe()?;
    check_image(pid, &[&own], &own)?;
    let sid = pipe::process_user_sid(pid).map_err(|e| os(&e))?;
    if sid != user_sid {
        return Err(Error::Untrusted(format!(
            "front {pid} runs as {sid}, not {user_sid}"
        )));
    }
    Ok(pid)
}

/// The front's check of the broker it connected to. Returns the broker's pid.
///
/// # Errors
/// [`Error::Untrusted`] when the server runs anything but this executable or the GUI next to
/// it; OS failures.
pub fn check_broker(pipe: &Pipe) -> Result<u32, Error> {
    let pid = pipe.peer_pid().map_err(|e| os(&e))?;
    let own = own_exe()?;
    let gui = own.with_file_name(GUI);
    check_image(pid, &[&own, &gui], &own)?;
    Ok(pid)
}

/// The process `pid` must run one of `allowed`, and, when that's `own`, the same bytes.
fn check_image(pid: u32, allowed: &[&Path], own: &Path) -> Result<(), Error> {
    let image = pipe::process_image(pid).map_err(|e| os(&e))?;
    let canonical = canonical(&image)?;
    let matched = allowed
        .iter()
        .find(|path| canonical_or_none(path).is_some_and(|p| p == canonical))
        .ok_or_else(|| {
            Error::Untrusted(format!(
                "process {pid} runs {}, not DISM Pro from {}",
                image.display(),
                own.parent().unwrap_or(own).display()
            ))
        })?;
    if *matched == own {
        let theirs = pipe::sha256_file(&image).map_err(|e| os(&e))?;
        let ours = pipe::sha256_file(own).map_err(|e| os(&e))?;
        if theirs != ours {
            return Err(Error::Untrusted(format!(
                "process {pid} runs {} with other contents than this one",
                image.display()
            )));
        }
    }
    Ok(())
}

fn own_exe() -> Result<PathBuf, Error> {
    let exe = std::env::current_exe()
        .map_err(|e| Error::Untrusted(format!("can't locate this executable: {e}")))?;
    canonical(&exe)
}

fn canonical(path: &Path) -> Result<PathBuf, Error> {
    canonical_or_none(path)
        .ok_or_else(|| Error::Untrusted(format!("can't resolve {}", path.display())))
}

/// Canonical paths compare reliably: same case, no short names, links resolved.
fn canonical_or_none(path: &Path) -> Option<PathBuf> {
    fs::canonicalize(path).ok()
}

fn os(e: &dpro_win::error::Error) -> Error {
    Error::Transport(e.to_string())
}

#[cfg(test)]
mod tests {
    use std::thread;

    use serde_json::{Value, json};

    use super::*;
    use crate::{
        Client, Handler, serve,
        wire::{Caller, Outcome},
    };

    struct Pong;

    impl Handler for Pong {
        fn call(&self, _tool: &str, args: Value, _caller: &Caller) -> Outcome {
            Outcome::Ok(args)
        }
    }

    /// Both ends in this test process: each sees the other running this same executable as
    /// this user, so both checks pass, and a call goes through over the real pipe.
    #[test]
    fn both_sides_accept_this_executable() {
        let sid = pipe::current_user_sid().unwrap();
        let name = format!("{}-{}", pipe_name(&sid, 0), std::process::id());
        let server = Pipe::create(&name, &sid).unwrap();
        let front_name = name.clone();
        let front = thread::spawn(move || {
            let p = Pipe::connect(&front_name, 2_000).unwrap();
            assert_eq!(check_broker(&p).unwrap(), std::process::id());
            let mut client = Client::hello(p).unwrap();
            client
                .call("ping", json!({ "n": 1 }), &Caller::default())
                .unwrap()
        });
        server.accept().unwrap();
        assert_eq!(check_front(&server, &sid).unwrap(), std::process::id());
        serve(&server, &Pong, std::process::id()).unwrap();
        assert_eq!(front.join().unwrap(), Outcome::Ok(json!({ "n": 1 })));
    }

    #[test]
    fn another_user_is_refused() {
        let sid = pipe::current_user_sid().unwrap();
        let name = format!("dpro-test-user-{}", std::process::id());
        let server = Pipe::create(&name, &sid).unwrap();
        let client_name = name.clone();
        let front = thread::spawn(move || Pipe::connect(&client_name, 2_000).unwrap());
        server.accept().unwrap();
        // LocalSystem: whoever runs the test, it isn't that.
        assert!(matches!(
            check_front(&server, "S-1-5-18"),
            Err(Error::Untrusted(_))
        ));
        drop(front.join().unwrap());
    }

    #[test]
    fn names_are_plain_pipe_names() {
        assert_eq!(
            pipe_name("S-1-5-21-1-2-3-1001", 1),
            "dismpro-agent-S-1-5-21-1-2-3-1001-1"
        );
    }
}
