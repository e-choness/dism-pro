//! `dismpro-cli mcp`: the unelevated MCP front on stdio (TD-0013 §5.1, §15). It answers the
//! protocol, lists the tools, runs the pure ones itself and passes the rest to the elevated
//! broker, which it finds or starts (one UAC prompt) on the first call that needs it.

use std::{
    cell::RefCell,
    io,
    path::PathBuf,
    time::{Duration, Instant},
};

use dpro_broker::{Client, peer, wire::Outcome};
use dpro_cli_fmt::{exit::ExitCode, messages::Messages};
use dpro_mcp::{
    catalog::Toolset,
    front::{Backend, Front, Seen, Unavailable},
    policy::Policy,
    protocol::Server,
    tool::{CallError, Caller},
};
use dpro_win::{
    elevate::{self, Elevated},
    pipe::{self, Pipe},
    system,
};
use serde_json::Value;

use super::{INFO, explain, parse_policy, policy_path, serve_lines};
use crate::error::CliError;

/// How long a connect waits for a busy broker pipe.
const CONNECT_WAIT_MS: u32 = 2_000;
/// How long a started broker has to open its pipe after the user accepts the prompt.
const START_TIMEOUT: Duration = Duration::from_secs(10);
/// `HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND)`: no pipe of that name, so no broker yet.
const NO_PIPE: u32 = 0x8007_0002;

/// Serves MCP on stdin/stdout until the client closes stdin.
///
/// # Errors
/// This process's identity can't be read, or stdin fails.
pub(crate) fn run(toolsets: Option<&[Toolset]>) -> Result<ExitCode, CliError> {
    let backend = PipeBackend {
        sid: pipe::current_user_sid()?,
        session: pipe::current_session_id()?,
        exe: std::env::current_exe()?,
        client: RefCell::new(None),
        m: Messages::en_us(),
    };
    let mut server = Server::new(INFO, Front::new(read_policy(), toolsets, backend));
    serve_lines(io::stdin().lock(), io::stdout().lock(), |line| {
        server.handle(line)
    })?;
    Ok(ExitCode::Ok)
}

/// The policy as this unelevated process can see it (TD-0013 §15). The broker reads it again,
/// with the checks that make it trustworthy; this copy only shapes the list and refuses early.
fn read_policy() -> Seen {
    let Some(root) = system::windows_directory()
        .ok()
        .and_then(|w| w.parent().map(std::path::Path::to_path_buf))
    else {
        return Seen::Unreadable;
    };
    match std::fs::read(policy_path(&root)) {
        Ok(bytes) => parse_policy(&bytes).map_or_else(Seen::Invalid, Seen::Policy),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Seen::Policy(Policy::default()),
        // Admin-only (written by hand), or unreadable for another reason: the broker decides.
        Err(_) => Seen::Unreadable,
    }
}

/// Reaches the broker over its pipe, starting it when there's none.
struct PipeBackend {
    sid: String,
    session: u32,
    exe: PathBuf,
    /// The connection, once made; dropped when it fails so the next call starts over.
    client: RefCell<Option<Client<Pipe>>>,
    m: Messages,
}

impl Backend for PipeBackend {
    fn broker(&self, tool: &str, args: Value, caller: &Caller) -> Result<Outcome, Unavailable> {
        let mut slot = self.client.borrow_mut();
        let mut client = match slot.take() {
            Some(c) => c,
            None => self.connect()?,
        };
        match client.call(tool, args, caller) {
            Ok(outcome) => {
                *slot = Some(client);
                Ok(outcome)
            }
            // The connection is dropped; the next call finds or starts a broker again.
            Err(e) => Err(Unavailable::Broker(format!(
                "the connection to the broker failed ({e}). The next call reconnects; a call \
                 that changes something may have run, so check before retrying."
            ))),
        }
    }

    fn local(&self, tool: &str, args: Value) -> Result<Value, CallError> {
        match tool {
            "explain_error" => {
                let text = args.get("code").and_then(Value::as_str).unwrap_or("");
                let code = explain::parse_code(text).map_err(CallError::Invalid)?;
                Ok(explain::explain(code, system::error_text, &self.m))
            }
            other => Err(CallError::UnknownTool(other.to_owned())),
        }
    }
}

impl PipeBackend {
    fn name(&self) -> String {
        peer::pipe_name(&self.sid, self.session)
    }

    /// The broker that's already there, or a new one started elevated.
    fn connect(&self) -> Result<Client<Pipe>, Unavailable> {
        let name = self.name();
        match Pipe::connect(&name, CONNECT_WAIT_MS) {
            Ok(pipe) => return trust(pipe),
            Err(e) if !no_pipe(&e) => {
                return Err(Unavailable::Broker(format!(
                    "couldn't open the broker's pipe: {e}"
                )));
            }
            Err(_) => {}
        }
        let args = ["--agent-broker", "--user-sid", self.sid.as_str()];
        let child = match elevate::run_elevated(&self.exe, &args) {
            Ok(Elevated::Started(child)) => child,
            Ok(Elevated::Declined) => return Err(Unavailable::ElevationDeclined),
            Err(e) => {
                return Err(Unavailable::Broker(format!(
                    "couldn't start the broker: {e}"
                )));
            }
        };
        let deadline = Instant::now() + START_TIMEOUT;
        loop {
            match Pipe::connect(&name, 250) {
                Ok(pipe) => return trust(pipe),
                Err(e) if !no_pipe(&e) => {
                    return Err(Unavailable::Broker(format!(
                        "couldn't open the broker's pipe: {e}"
                    )));
                }
                Err(_) => {}
            }
            match child.exit_code() {
                // It found another broker serving this session, which is there to connect to.
                Ok(Some(0)) => {
                    return Pipe::connect(&name, CONNECT_WAIT_MS)
                        .map_err(|e| {
                            Unavailable::Broker(format!("couldn't open the broker's pipe: {e}"))
                        })
                        .and_then(trust);
                }
                Ok(Some(code)) => {
                    return Err(Unavailable::Broker(format!(
                        "the broker exited with code {code} before it was ready"
                    )));
                }
                Ok(None) => {}
                Err(e) => return Err(Unavailable::Broker(format!("lost the broker: {e}"))),
            }
            if Instant::now() >= deadline {
                return Err(Unavailable::Broker(format!(
                    "the broker didn't open its pipe within {} s",
                    START_TIMEOUT.as_secs()
                )));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// Checks that DISM Pro serves the pipe (not a squatter), then says hello.
fn trust(pipe: Pipe) -> Result<Client<Pipe>, Unavailable> {
    peer::check_broker(&pipe).map_err(|e| {
        Unavailable::Broker(format!(
            "the process serving the broker's pipe isn't DISM Pro's broker ({e})"
        ))
    })?;
    Client::hello(pipe).map_err(|e| Unavailable::Broker(format!("the broker didn't answer: {e}")))
}

fn no_pipe(e: &dpro_win::Error) -> bool {
    matches!(e, dpro_win::Error::Os(os) if os.hresult_bits() == NO_PIPE)
}
