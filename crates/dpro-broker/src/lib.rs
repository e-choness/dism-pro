//! Named-pipe IPC between the unelevated MCP front and the elevated agent broker (TD-0013 §5.1,
//! ADR-0006).
//!
//! Layer: platform. OS access goes through `dpro-win`.
//! Allowed workspace dependencies: `dpro-core`, `dpro-win`.
//!
//! - [`wire`]: the messages, one JSON object per pipe message.
//! - [`Transport`]: what carries them; [`memory`] for tests, the broker pipe on Windows.
//! - [`serve`] runs the broker's side of one connection; [`Client`] is the front's.
//! - `peer` (Windows): who may be on the other end.
//!
//! The tools themselves live above this crate: the front's catalog in `dpro-mcp`, the broker's
//! [`Handler`] in `dismpro-cli`, which may use the engine.

#![forbid(unsafe_code)]

pub mod memory;
#[cfg(windows)]
pub mod peer;
#[cfg(windows)]
mod pipe;
pub mod wire;

use serde_json::Value;

use crate::wire::{Caller, Outcome, Request, Response, VERSION};

/// Carries whole messages. Message boundaries are the transport's (the pipe is in message
/// mode), so there's no framing.
pub trait Transport {
    /// Sends one message.
    ///
    /// # Errors
    /// The transport failed or the peer is gone.
    fn send(&self, message: &[u8]) -> Result<(), Error>;

    /// Receives one message into `buf`. `Ok(false)` means the peer closed its end.
    ///
    /// # Errors
    /// The transport failed.
    fn recv(&self, buf: &mut Vec<u8>) -> Result<bool, Error>;
}

/// What the broker does with a call. `dismpro-cli` implements it with the engine and the policy.
pub trait Handler {
    fn call(&self, tool: &str, args: Value, caller: &Caller) -> Outcome;
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("broker pipe: {0}")]
    Transport(String),
    #[error("malformed broker message: {0}")]
    Malformed(#[from] serde_json::Error),
    #[error("the other end speaks broker protocol {theirs}, this one {ours}")]
    Version { ours: u32, theirs: u32 },
    #[error("the broker refused the connection: {0}")]
    Refused(String),
    #[error("the broker closed the connection")]
    Closed,
    #[error("unexpected broker message: {0}")]
    Unexpected(&'static str),
    /// The process on the other end isn't one this side trusts.
    #[error("untrusted peer: {0}")]
    Untrusted(String),
}

/// Serves one front until it disconnects: its hello, then its calls in order.
///
/// # Errors
/// Transport failures; a hello that's missing or from another version (refused, then `Err`); a
/// malformed message (the connection is dropped: the front is the same binary, so it's a bug).
pub fn serve(
    transport: &impl Transport,
    handler: &impl Handler,
    broker_pid: u32,
) -> Result<(), Error> {
    let mut buf = Vec::with_capacity(4096);
    if !transport.recv(&mut buf)? {
        return Ok(());
    }
    match serde_json::from_slice::<Request>(&buf) {
        Ok(Request::Hello { version }) if version == VERSION => send(
            transport,
            &Response::Hello {
                version: VERSION,
                broker_pid,
            },
        )?,
        Ok(Request::Hello { version }) => {
            let reason = format!("broker protocol {version} isn't {VERSION}");
            send(transport, &Response::Refused { reason })?;
            return Err(Error::Version {
                ours: VERSION,
                theirs: version,
            });
        }
        Ok(Request::Call { .. }) | Err(_) => {
            let reason = "the first message must be a hello".to_owned();
            send(transport, &Response::Refused { reason })?;
            return Err(Error::Unexpected("a call before the hello"));
        }
    }
    while transport.recv(&mut buf)? {
        match serde_json::from_slice::<Request>(&buf)? {
            Request::Call {
                id,
                tool,
                args,
                caller,
            } => {
                let outcome = handler.call(&tool, args, &caller);
                send(transport, &Response::Result { id, outcome })?;
            }
            Request::Hello { .. } => return Err(Error::Unexpected("a second hello")),
        }
    }
    Ok(())
}

/// The front's end of one connection.
#[derive(Debug)]
pub struct Client<T> {
    transport: T,
    broker_pid: u32,
    next_id: u64,
    buf: Vec<u8>,
}

impl<T: Transport> Client<T> {
    /// Says hello over a connected transport.
    ///
    /// # Errors
    /// Refused (another version), closed, transport failures.
    pub fn hello(transport: T) -> Result<Self, Error> {
        let mut client = Self {
            transport,
            broker_pid: 0,
            next_id: 1,
            buf: Vec::with_capacity(4096),
        };
        send(&client.transport, &Request::Hello { version: VERSION })?;
        match client.receive()? {
            Response::Hello {
                version,
                broker_pid,
            } if version == VERSION => {
                client.broker_pid = broker_pid;
                Ok(client)
            }
            Response::Hello { version, .. } => Err(Error::Version {
                ours: VERSION,
                theirs: version,
            }),
            Response::Refused { reason } => Err(Error::Refused(reason)),
            Response::Result { .. } => Err(Error::Unexpected("a result before the hello")),
        }
    }

    /// The pid the broker reported in its hello (the front checks the pipe's own answer too).
    #[must_use]
    pub fn broker_pid(&self) -> u32 {
        self.broker_pid
    }

    /// Runs one tool in the broker and waits for its outcome.
    ///
    /// # Errors
    /// Transport failures, a closed broker, a reply for another call.
    pub fn call(&mut self, tool: &str, args: Value, caller: &Caller) -> Result<Outcome, Error> {
        let id = self.next_id;
        self.next_id += 1;
        send(
            &self.transport,
            &Request::Call {
                id,
                tool: tool.to_owned(),
                args,
                caller: caller.clone(),
            },
        )?;
        match self.receive()? {
            Response::Result { id: got, outcome } if got == id => Ok(outcome),
            Response::Result { .. } => Err(Error::Unexpected("a result for another call")),
            Response::Refused { reason } => Err(Error::Refused(reason)),
            Response::Hello { .. } => Err(Error::Unexpected("a hello after the hello")),
        }
    }

    fn receive(&mut self) -> Result<Response, Error> {
        if !self.transport.recv(&mut self.buf)? {
            return Err(Error::Closed);
        }
        Ok(serde_json::from_slice(&self.buf)?)
    }
}

fn send(transport: &impl Transport, message: &impl serde::Serialize) -> Result<(), Error> {
    transport.send(&serde_json::to_vec(message)?)
}

#[cfg(test)]
mod tests {
    use std::thread;

    use serde_json::json;

    use super::*;
    use crate::memory;

    struct Echo;

    impl Handler for Echo {
        fn call(&self, tool: &str, args: Value, caller: &Caller) -> Outcome {
            match tool {
                "echo" => Outcome::Ok(json!({ "args": args, "client": caller.client_name })),
                "fail" => Outcome::Failed("it failed".to_owned()),
                other => Outcome::UnknownTool(other.to_owned()),
            }
        }
    }

    fn caller() -> Caller {
        Caller {
            protocol_version: "2026-07-28".to_owned(),
            client_name: Some("test".to_owned()),
            client_version: None,
        }
    }

    #[test]
    fn calls_reach_the_handler_in_order() {
        let (front, broker) = memory::pair();
        let server = thread::spawn(move || serve(&broker, &Echo, 4242));
        let mut client = Client::hello(front).unwrap();
        assert_eq!(client.broker_pid(), 4242);
        for n in 0..3 {
            let out = client.call("echo", json!({ "n": n }), &caller()).unwrap();
            assert_eq!(
                out,
                Outcome::Ok(json!({ "args": { "n": n }, "client": "test" }))
            );
        }
        assert_eq!(
            client.call("fail", Value::Null, &caller()).unwrap(),
            Outcome::Failed("it failed".to_owned())
        );
        assert_eq!(
            client.call("nope", Value::Null, &caller()).unwrap(),
            Outcome::UnknownTool("nope".to_owned())
        );
        drop(client);
        server.join().unwrap().unwrap();
    }

    #[test]
    fn another_version_is_refused() {
        let (front, broker) = memory::pair();
        let server = thread::spawn(move || serve(&broker, &Echo, 1));
        send(&front, &Request::Hello { version: 999 }).unwrap();
        let mut buf = Vec::new();
        assert!(front.recv(&mut buf).unwrap());
        assert!(matches!(
            serde_json::from_slice(&buf).unwrap(),
            Response::Refused { .. }
        ));
        assert!(matches!(
            server.join().unwrap(),
            Err(Error::Version { theirs: 999, .. })
        ));
    }

    #[test]
    fn a_call_before_the_hello_is_refused() {
        let (front, broker) = memory::pair();
        let server = thread::spawn(move || serve(&broker, &Echo, 1));
        let call = Request::Call {
            id: 1,
            tool: "echo".to_owned(),
            args: Value::Null,
            caller: caller(),
        };
        send(&front, &call).unwrap();
        let mut buf = Vec::new();
        assert!(front.recv(&mut buf).unwrap());
        assert!(matches!(
            serde_json::from_slice(&buf).unwrap(),
            Response::Refused { .. }
        ));
        assert!(server.join().unwrap().is_err());
    }

    #[test]
    fn garbage_drops_the_connection() {
        let (front, broker) = memory::pair();
        let server = thread::spawn(move || serve(&broker, &Echo, 1));
        let mut client = Client::hello(front).unwrap();
        client.transport.send(b"{not json").unwrap();
        assert!(matches!(server.join().unwrap(), Err(Error::Malformed(_))));
        assert!(matches!(
            client.call("echo", Value::Null, &caller()),
            Err(Error::Closed | Error::Transport(_))
        ));
    }

    #[test]
    fn a_front_that_leaves_ends_the_service_cleanly() {
        let (front, broker) = memory::pair();
        let server = thread::spawn(move || serve(&broker, &Echo, 1));
        drop(Client::hello(front).unwrap());
        server.join().unwrap().unwrap();
    }
}
