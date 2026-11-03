//! Messages between the front and the broker. Each is one JSON object in one pipe message.
//!
//! ```text
//! front  → broker   {"type":"hello","version":1}
//! broker → front    {"type":"hello","version":1,"broker_pid":4242}   or {"type":"refused",…}
//! front  → broker   {"type":"call","id":1,"tool":"targets_list","args":{…},"caller":{…}}
//! broker → front    {"type":"result","id":1,"outcome":{"ok":{…}}}
//! ```

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Bumped whenever a message changes shape. Front and broker are normally the same binary (each
/// checks the other's image), so a mismatch means a broker left running by another version.
pub const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Hello {
        version: u32,
    },
    Call {
        id: u64,
        tool: String,
        args: Value,
        caller: Caller,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Hello { version: u32, broker_pid: u32 },
    Refused { reason: String },
    Result { id: u64, outcome: Outcome },
}

/// Who made a call, as the front learned it from the MCP client (TD-0013 §8.3: the client's
/// name and version are its own claim, recorded as unverified).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Caller {
    pub protocol_version: String,
    pub client_name: Option<String>,
    pub client_version: Option<String>,
}

/// A tool's outcome. `Invalid` and `Failed` reach the model as a tool error it can act on;
/// `UnknownTool` is a protocol error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Ok(Value),
    UnknownTool(String),
    /// The arguments don't fit the tool's input schema.
    Invalid(String),
    Failed(String),
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn messages_have_the_documented_shape() {
        let call = Request::Call {
            id: 1,
            tool: "targets_list".to_owned(),
            args: json!({}),
            caller: Caller::default(),
        };
        let v = serde_json::to_value(&call).unwrap();
        assert_eq!(v["type"], "call");
        assert_eq!(v["tool"], "targets_list");
        let result = Response::Result {
            id: 1,
            outcome: Outcome::Ok(json!({ "n": 1 })),
        };
        assert_eq!(
            serde_json::to_value(&result).unwrap(),
            json!({ "type": "result", "id": 1, "outcome": { "ok": { "n": 1 } } })
        );
        assert_eq!(
            serde_json::to_value(Response::Hello {
                version: VERSION,
                broker_pid: 7
            })
            .unwrap(),
            json!({ "type": "hello", "version": 1, "broker_pid": 7 })
        );
    }

    #[test]
    fn round_trips() {
        for outcome in [
            Outcome::Ok(json!([1, 2])),
            Outcome::UnknownTool("x".to_owned()),
            Outcome::Invalid("bad".to_owned()),
            Outcome::Failed("no".to_owned()),
        ] {
            let r = Response::Result { id: 9, outcome };
            let back: Response = serde_json::from_slice(&serde_json::to_vec(&r).unwrap()).unwrap();
            assert_eq!(back, r);
        }
    }
}
