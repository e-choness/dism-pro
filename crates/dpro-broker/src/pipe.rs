//! The broker pipe as a [`Transport`].

use dpro_win::{error::Error as WinError, pipe::Pipe};

use crate::{Error, Transport};

/// `HRESULT`s of a peer that went away: `ERROR_BROKEN_PIPE`, `ERROR_NO_DATA` (the pipe is being
/// closed) and `ERROR_PIPE_NOT_CONNECTED`.
const GONE: [u32; 3] = [0x8007_006D, 0x8007_00E8, 0x8007_00E9];

impl Transport for Pipe {
    fn send(&self, message: &[u8]) -> Result<(), Error> {
        Pipe::send(self, message).map_err(|e| {
            if gone(&e) {
                Error::Closed
            } else {
                Error::Transport(e.to_string())
            }
        })
    }

    fn recv(&self, buf: &mut Vec<u8>) -> Result<bool, Error> {
        match Pipe::recv(self, buf) {
            Ok(()) => Ok(true),
            Err(e) if gone(&e) => Ok(false),
            Err(e) => Err(Error::Transport(e.to_string())),
        }
    }
}

fn gone(e: &WinError) -> bool {
    matches!(e, WinError::Os(os) if GONE.contains(&u32::from_ne_bytes(os.code.to_ne_bytes())))
}
