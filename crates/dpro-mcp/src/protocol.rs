//! MCP over JSON-RPC 2.0, both protocol generations in one stdio process (TD-0013 §15):
//! - an `initialize` request selects **legacy** semantics for the rest of the process: 2025-11-25,
//!   negotiating down to 2025-06-18;
//! - a request whose `params._meta` names `io.modelcontextprotocol/protocolVersion` is served
//!   **statelessly** as 2026-07-28, whatever came before it.
//!
//! [`Server::handle`] maps one input line to at most one output line and does no I/O, so the
//! protocol is tested with transcripts. The stdio loop belongs to the caller.

use serde_json::{Map, Value, json};

use crate::tool::{CallError, Caller, Tools};

/// The stateless generation.
pub const MODERN: &str = "2026-07-28";
/// The `initialize` generation, newest first; the first is the one offered on a mismatch.
pub const LEGACY: [&str; 2] = ["2025-11-25", "2025-06-18"];

const META_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
const META_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";
const META_CLIENT: &str = "io.modelcontextprotocol/clientInfo";
const META_SERVER: &str = "io.modelcontextprotocol/serverInfo";
/// How long a 2026-07-28 client may cache `tools/list` and `server/discover`. The catalog changes
/// only when an administrator edits the agent policy.
const TTL_MS: u64 = 5 * 60 * 1000;
/// Client-supplied strings kept for the audit are cut to this many bytes.
const MAX_CLIENT_FIELD: usize = 128;

/// JSON-RPC and MCP error codes.
pub mod code {
    pub const PARSE: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const UNSUPPORTED_VERSION: i64 = -32022;
}

/// What the server says about itself.
#[derive(Debug, Clone, Copy)]
pub struct ServerInfo {
    pub name: &'static str,
    pub version: &'static str,
    /// Shown to the model by clients that support it.
    pub instructions: &'static str,
}

/// A JSON-RPC error.
#[derive(Debug, Clone, PartialEq)]
struct RpcError {
    code: i64,
    message: String,
    data: Option<Value>,
}

impl RpcError {
    fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }
}

/// Which generation a request is served under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Era {
    Legacy,
    Modern,
}

/// One MCP server: the tools, plus the legacy client once it has sent `initialize`.
#[derive(Debug)]
pub struct Server<T> {
    info: ServerInfo,
    tools: T,
    legacy: Option<Caller>,
}

impl<T: Tools> Server<T> {
    pub fn new(info: ServerInfo, tools: T) -> Self {
        Self {
            info,
            tools,
            legacy: None,
        }
    }

    /// The legacy client, once `initialize` has named it.
    #[must_use]
    pub fn legacy_client(&self) -> Option<&Caller> {
        self.legacy.as_ref()
    }

    /// Handles one line of input. Returns the response line (no newline; `serde_json` never writes
    /// one inside a value), or `None` for notifications and for responses the client sends.
    pub fn handle(&mut self, line: &str) -> Option<String> {
        let message: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                let err = RpcError::new(code::PARSE, format!("parse error: {e}"));
                return Some(error_line(&Value::Null, &err));
            }
        };
        let Value::Object(message) = message else {
            // Batches were removed in 2025-06-18.
            let err = RpcError::new(code::INVALID_REQUEST, "expected one JSON-RPC object");
            return Some(error_line(&Value::Null, &err));
        };
        let id = message.get("id");
        if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            let err = RpcError::new(code::INVALID_REQUEST, "jsonrpc must be \"2.0\"");
            return Some(error_line(id.unwrap_or(&Value::Null), &err));
        }
        let method = message.get("method").and_then(Value::as_str);
        match (id, method) {
            // A notification: `initialized` and `cancelled` need no answer, and calls run to
            // completion before the next line is read.
            (None, Some(_)) => None,
            // A response to a server request; this server sends none.
            (Some(_), None) if message.contains_key("result") || message.contains_key("error") => {
                None
            }
            (Some(id @ (Value::String(_) | Value::Number(_))), Some(method)) => {
                let params = message.get("params").cloned().unwrap_or(json!({}));
                Some(match self.request(method, params) {
                    Ok(result) => result_line(id, &result),
                    Err(err) => error_line(id, &err),
                })
            }
            _ => {
                let err = RpcError::new(code::INVALID_REQUEST, "not a JSON-RPC request");
                Some(error_line(id.unwrap_or(&Value::Null), &err))
            }
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, RpcError> {
        let Value::Object(params) = params else {
            return Err(RpcError::new(
                code::INVALID_PARAMS,
                "params must be an object",
            ));
        };
        if method == "initialize" {
            return Ok(self.initialize(&params));
        }
        let meta = params.get("_meta").and_then(Value::as_object);
        if let Some(version) = meta.and_then(|m| m.get(META_VERSION)) {
            let caller = modern_caller(version, meta)?;
            let mut result = match method {
                "server/discover" => self.discover(),
                "tools/list" => self.tools_list(Era::Modern),
                "tools/call" => self.tools_call(&params, &caller, Era::Modern)?,
                _ => return Err(not_found(method)),
            };
            if let Value::Object(r) = &mut result {
                r.insert("_meta".into(), json!({ META_SERVER: self.server_info() }));
            }
            return Ok(result);
        }
        let Some(caller) = self.legacy.clone() else {
            return Err(RpcError::new(
                code::INVALID_PARAMS,
                format!(
                    "missing _meta[\"{META_VERSION}\"]: send server/discover (2026-07-28) or \
                     initialize (2025-11-25) first"
                ),
            ));
        };
        match method {
            "ping" | "logging/setLevel" => Ok(json!({})),
            "tools/list" => Ok(self.tools_list(Era::Legacy)),
            "tools/call" => self.tools_call(&params, &caller, Era::Legacy),
            _ => Err(not_found(method)),
        }
    }

    /// Legacy handshake: echo a version both sides support, or offer the newest legacy one.
    fn initialize(&mut self, params: &Map<String, Value>) -> Value {
        let requested = params
            .get("protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let version = LEGACY
            .iter()
            .find(|&&v| v == requested)
            .copied()
            .unwrap_or(LEGACY[0]);
        let client = params.get("clientInfo");
        self.legacy = Some(Caller {
            protocol_version: version.to_owned(),
            client_name: client_field(client, "name"),
            client_version: client_field(client, "version"),
        });
        json!({
            "protocolVersion": version,
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": self.server_info(),
            "instructions": self.info.instructions,
        })
    }

    fn discover(&self) -> Value {
        json!({
            "resultType": "complete",
            "supportedVersions": supported(),
            "capabilities": { "tools": {} },
            "instructions": self.info.instructions,
            "ttlMs": TTL_MS,
            "cacheScope": "public",
        })
    }

    fn tools_list(&self, era: Era) -> Value {
        let tools = serde_json::to_value(self.tools.list()).unwrap_or_else(|_| json!([]));
        match era {
            Era::Legacy => json!({ "tools": tools }),
            Era::Modern => json!({
                "resultType": "complete",
                "tools": tools,
                "ttlMs": TTL_MS,
                "cacheScope": "public",
            }),
        }
    }

    fn tools_call(
        &self,
        params: &Map<String, Value>,
        caller: &Caller,
        era: Era,
    ) -> Result<Value, RpcError> {
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            return Err(RpcError::new(
                code::INVALID_PARAMS,
                "tools/call needs a name",
            ));
        };
        let args = match params.get("arguments") {
            None | Some(Value::Null) => json!({}),
            Some(a @ Value::Object(_)) => a.clone(),
            Some(_) => {
                return Err(RpcError::new(
                    code::INVALID_PARAMS,
                    "arguments must be an object",
                ));
            }
        };
        let mut result = match self.tools.call(name, args, caller) {
            // The model reads `content`: one compact copy of the structured result (TD-0013 §15).
            Ok(structured) => json!({
                "content": [{ "type": "text", "text": structured.to_string() }],
                "structuredContent": structured,
                "isError": false,
            }),
            Err(CallError::UnknownTool(name)) => {
                return Err(RpcError::new(
                    code::INVALID_PARAMS,
                    format!("unknown tool: {name}"),
                ));
            }
            Err(CallError::Invalid(msg) | CallError::Failed(msg)) => json!({
                "content": [{ "type": "text", "text": msg }],
                "isError": true,
            }),
        };
        if era == Era::Modern
            && let Value::Object(r) = &mut result
        {
            r.insert("resultType".into(), json!("complete"));
        }
        Ok(result)
    }

    fn server_info(&self) -> Value {
        json!({ "name": self.info.name, "version": self.info.version })
    }
}

/// Every version this server speaks, newest first.
fn supported() -> Vec<&'static str> {
    std::iter::once(MODERN).chain(LEGACY).collect()
}

/// The caller of a 2026-07-28 request, or why the request is malformed.
fn modern_caller(version: &Value, meta: Option<&Map<String, Value>>) -> Result<Caller, RpcError> {
    let Some(version) = version.as_str() else {
        return Err(RpcError::new(
            code::INVALID_PARAMS,
            format!("{META_VERSION} must be a string"),
        ));
    };
    if version != MODERN {
        return Err(RpcError {
            code: code::UNSUPPORTED_VERSION,
            message: "Unsupported protocol version".into(),
            data: Some(json!({ "supported": supported(), "requested": version })),
        });
    }
    if !meta
        .and_then(|m| m.get(META_CAPABILITIES))
        .is_some_and(Value::is_object)
    {
        return Err(RpcError::new(
            code::INVALID_PARAMS,
            format!("missing _meta[\"{META_CAPABILITIES}\"]"),
        ));
    }
    let client = meta.and_then(|m| m.get(META_CLIENT));
    Ok(Caller {
        protocol_version: MODERN.to_owned(),
        client_name: client_field(client, "name"),
        client_version: client_field(client, "version"),
    })
}

/// A string field of a client's `Implementation`, cut to [`MAX_CLIENT_FIELD`] bytes and stripped of
/// control characters: it ends up in the journal and in approval prompts (TD-0013 §8.1).
fn client_field(client: Option<&Value>, key: &str) -> Option<String> {
    let s = client?.get(key)?.as_str()?;
    let mut out: String = s.chars().filter(|c| !c.is_control()).collect();
    if out.len() > MAX_CLIENT_FIELD {
        let mut cut = MAX_CLIENT_FIELD;
        while !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out.truncate(cut);
    }
    Some(out)
}

fn not_found(method: &str) -> RpcError {
    RpcError::new(
        code::METHOD_NOT_FOUND,
        format!("method not found: {method}"),
    )
}

fn result_line(id: &Value, result: &Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string()
}

fn error_line(id: &Value, err: &RpcError) -> String {
    let mut error = json!({ "code": err.code, "message": err.message });
    if let (Some(data), Value::Object(e)) = (&err.data, &mut error) {
        e.insert("data".into(), data.clone());
    }
    json!({ "jsonrpc": "2.0", "id": id, "error": error }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::{Annotations, ToolDef};

    /// One tool that echoes its `text` argument, and one that always fails.
    struct Echo(Vec<ToolDef>);

    impl Echo {
        fn new() -> Self {
            let def = |name| ToolDef {
                name,
                title: "Echo",
                description: "Returns its text.",
                input_schema: json!({ "type": "object", "properties": { "text": { "type": "string" } } }),
                output_schema: None,
                annotations: Annotations::READ_ONLY,
            };
            Self(vec![def("echo"), def("broken")])
        }
    }

    impl Tools for Echo {
        fn list(&self) -> &[ToolDef] {
            &self.0
        }

        fn call(&self, name: &str, args: Value, caller: &Caller) -> Result<Value, CallError> {
            match name {
                "echo" => match args.get("text").and_then(Value::as_str) {
                    Some(t) => Ok(json!({ "text": t, "client": caller.client_name })),
                    None => Err(CallError::Invalid("text is required".into())),
                },
                "broken" => Err(CallError::Failed("it broke".into())),
                other => Err(CallError::UnknownTool(other.into())),
            }
        }
    }

    const INFO: ServerInfo = ServerInfo {
        name: "dismpro",
        version: "0.1.0",
        instructions: "Plan before apply.",
    };

    fn server() -> Server<Echo> {
        Server::new(INFO, Echo::new())
    }

    fn ask(s: &mut Server<Echo>, line: &Value) -> Value {
        let out = s.handle(&line.to_string()).unwrap();
        assert!(!out.contains('\n'));
        serde_json::from_str(&out).unwrap()
    }

    fn modern(id: u64, method: &str, mut params: Value) -> Value {
        params["_meta"] = json!({
            META_VERSION: MODERN,
            META_CAPABILITIES: {},
            META_CLIENT: { "name": "test-client", "version": "1.0" },
        });
        json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
    }

    fn legacy(id: u64, method: &str, params: &Value) -> Value {
        json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
    }

    #[test]
    fn modern_discover_list_and_call_need_no_handshake() {
        let mut s = server();
        let discover = ask(&mut s, &modern(1, "server/discover", json!({})));
        assert_eq!(discover["id"], 1);
        assert_eq!(discover["result"]["resultType"], "complete");
        assert_eq!(
            discover["result"]["supportedVersions"],
            json!(["2026-07-28", "2025-11-25", "2025-06-18"])
        );
        assert_eq!(discover["result"]["_meta"][META_SERVER]["name"], "dismpro");
        let list = ask(&mut s, &modern(2, "tools/list", json!({})));
        assert_eq!(list["result"]["tools"][0]["name"], "echo");
        assert_eq!(
            list["result"]["tools"][0]["annotations"]["readOnlyHint"],
            true
        );
        assert_eq!(list["result"]["cacheScope"], "public");
        let call = ask(
            &mut s,
            &modern(
                3,
                "tools/call",
                json!({ "name": "echo", "arguments": { "text": "hi" } }),
            ),
        );
        let result = &call["result"];
        assert_eq!(result["resultType"], "complete");
        assert_eq!(result["isError"], false);
        assert_eq!(result["structuredContent"]["client"], "test-client");
        let text: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(text, result["structuredContent"]);
        assert!(
            s.legacy_client().is_none(),
            "modern requests leave no state"
        );
    }

    #[test]
    fn legacy_initialize_then_list_and_call() {
        let mut s = server();
        let init = json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "old\u{7}-client", "version": "0.9" },
        });
        let i = ask(&mut s, &legacy(1, "initialize", &init));
        assert_eq!(i["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(i["result"]["serverInfo"]["version"], "0.1.0");
        assert_eq!(
            s.handle(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#),
            None
        );
        assert_eq!(
            ask(&mut s, &legacy(2, "ping", &json!({})))["result"],
            json!({})
        );
        let l = ask(&mut s, &legacy(3, "tools/list", &json!({})));
        assert!(l["result"].get("resultType").is_none());
        assert_eq!(l["result"]["tools"].as_array().unwrap().len(), 2);
        let c = ask(
            &mut s,
            &legacy(
                4,
                "tools/call",
                &json!({ "name": "echo", "arguments": { "text": "x" } }),
            ),
        );
        assert!(c["result"].get("resultType").is_none());
        // Control characters are stripped from what the client says about itself.
        assert_eq!(c["result"]["structuredContent"]["client"], "old-client");
    }

    #[test]
    fn legacy_negotiates_to_its_newest_version() {
        let mut s = server();
        let i = ask(
            &mut s,
            &legacy(1, "initialize", &json!({ "protocolVersion": "2024-11-05" })),
        );
        assert_eq!(i["result"]["protocolVersion"], "2025-11-25");
    }

    #[test]
    fn one_process_serves_both_eras() {
        let mut s = server();
        ask(
            &mut s,
            &legacy(1, "initialize", &json!({ "protocolVersion": "2025-11-25" })),
        );
        let m = ask(&mut s, &modern(2, "tools/list", json!({})));
        assert_eq!(m["result"]["resultType"], "complete");
        let l = ask(&mut s, &legacy(3, "tools/list", &json!({})));
        assert!(l["result"].get("resultType").is_none());
    }

    #[test]
    fn unsupported_version_lists_the_supported_ones() {
        let mut s = server();
        let mut req = modern(7, "tools/list", json!({}));
        req["params"]["_meta"][META_VERSION] = json!("1900-01-01");
        let e = ask(&mut s, &req);
        assert_eq!(e["id"], 7);
        assert_eq!(e["error"]["code"], code::UNSUPPORTED_VERSION);
        assert_eq!(e["error"]["data"]["requested"], "1900-01-01");
        assert_eq!(e["error"]["data"]["supported"][0], MODERN);
    }

    #[test]
    fn malformed_requests_get_the_right_errors() {
        let mut s = server();
        let parse = ask_raw(&mut s, "{not json");
        assert_eq!(parse["error"]["code"], code::PARSE);
        assert_eq!(parse["id"], Value::Null);
        let batch = ask_raw(&mut s, "[]");
        assert_eq!(batch["error"]["code"], code::INVALID_REQUEST);
        let version = ask_raw(&mut s, r#"{"jsonrpc":"1.0","id":1,"method":"ping"}"#);
        assert_eq!(version["error"]["code"], code::INVALID_REQUEST);
        let null_id = ask_raw(
            &mut s,
            r#"{"jsonrpc":"2.0","id":null,"method":"tools/list"}"#,
        );
        assert_eq!(null_id["error"]["code"], code::INVALID_REQUEST);
        // Neither era established.
        let bare = ask(&mut s, &legacy(2, "tools/list", &json!({})));
        assert_eq!(bare["error"]["code"], code::INVALID_PARAMS);
        // Modern without capabilities.
        let mut no_caps = modern(3, "tools/list", json!({}));
        no_caps["params"]["_meta"]
            .as_object_mut()
            .unwrap()
            .remove(META_CAPABILITIES);
        assert_eq!(ask(&mut s, &no_caps)["error"]["code"], code::INVALID_PARAMS);
        // `ping` is gone in 2026-07-28.
        let ping = ask(&mut s, &modern(4, "ping", json!({})));
        assert_eq!(ping["error"]["code"], code::METHOD_NOT_FOUND);
        let unknown = ask(&mut s, &modern(5, "tools/call", json!({ "name": "nope" })));
        assert_eq!(unknown["error"]["code"], code::INVALID_PARAMS);
        let bad_args = ask(
            &mut s,
            &modern(6, "tools/call", json!({ "name": "echo", "arguments": [1] })),
        );
        assert_eq!(bad_args["error"]["code"], code::INVALID_PARAMS);
    }

    #[test]
    fn tool_failures_are_results_the_model_can_read() {
        let mut s = server();
        let invalid = ask(&mut s, &modern(1, "tools/call", json!({ "name": "echo" })));
        assert_eq!(invalid["result"]["isError"], true);
        assert_eq!(invalid["result"]["content"][0]["text"], "text is required");
        let failed = ask(
            &mut s,
            &modern(2, "tools/call", json!({ "name": "broken" })),
        );
        assert_eq!(failed["result"]["isError"], true);
        assert!(failed["result"].get("structuredContent").is_none());
    }

    #[test]
    fn notifications_and_client_responses_get_no_answer() {
        let mut s = server();
        for line in [
            r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}"#,
            r#"{"jsonrpc":"2.0","id":9,"result":{}}"#,
        ] {
            assert_eq!(s.handle(line), None, "{line}");
        }
    }

    #[test]
    fn long_client_names_are_cut_on_a_char_boundary() {
        let long = "é".repeat(100);
        let v = json!({ "name": long });
        let cut = client_field(Some(&v), "name").unwrap();
        assert!(cut.len() <= MAX_CLIENT_FIELD);
        assert!(cut.chars().all(|c| c == 'é'));
    }

    fn ask_raw(s: &mut Server<Echo>, line: &str) -> Value {
        serde_json::from_str(&s.handle(line).unwrap()).unwrap()
    }
}
