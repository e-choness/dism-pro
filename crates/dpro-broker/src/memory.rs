//! An in-process [`Transport`] pair: what the pipe does, for tests on any OS.

use std::sync::{
    Mutex,
    mpsc::{Receiver, Sender, channel},
};

use crate::{Error, Transport};

/// One end of a [`pair`].
#[derive(Debug)]
pub struct End {
    tx: Sender<Vec<u8>>,
    rx: Mutex<Receiver<Vec<u8>>>,
}

/// Two connected ends. Dropping one closes the other's receiving side.
#[must_use]
pub fn pair() -> (End, End) {
    let (a_tx, b_rx) = channel();
    let (b_tx, a_rx) = channel();
    (
        End {
            tx: a_tx,
            rx: Mutex::new(a_rx),
        },
        End {
            tx: b_tx,
            rx: Mutex::new(b_rx),
        },
    )
}

impl Transport for End {
    fn send(&self, message: &[u8]) -> Result<(), Error> {
        self.tx.send(message.to_vec()).map_err(|_| Error::Closed)
    }

    fn recv(&self, buf: &mut Vec<u8>) -> Result<bool, Error> {
        let rx = self
            .rx
            .lock()
            .map_err(|_| Error::Transport("poisoned receiver".to_owned()))?;
        match rx.recv() {
            Ok(message) => {
                buf.clear();
                buf.extend_from_slice(&message);
                Ok(true)
            }
            Err(_) => Ok(false),
        }
    }
}
