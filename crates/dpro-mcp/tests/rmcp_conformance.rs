//! Conformance against `rmcp`, the reference Rust MCP client (TD-0013 §12). The client talks to
//! DISM Pro's real protocol layer and front, in both protocol eras, lists the catalog and calls
//! every tool. A fake backend stands in for the broker, so this runs anywhere (T1).

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers: any failure fails the test"
)]

use dpro_broker::wire::Outcome;
use dpro_mcp::{
    catalog::{self, Tier, Toolset},
    front::{Backend, Front, Seen, Unavailable},
    policy::Policy,
    protocol::{LEGACY, MODERN, Server, ServerInfo},
    tool::{CallError, Caller},
};
use rmcp::{
    ClientHandler, ClientLifecycleMode, ClientServiceExt, RoleClient,
    model::{CallToolRequestParams, ProtocolVersion},
    service::RunningService,
};
use serde_json::{Map, Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};

const INFO: ServerInfo = ServerInfo {
    name: "dismpro",
    version: "0.1.0",
    instructions: "Conformance test server.",
};

/// Answers every broker call with what it was asked, so the test can check what arrived.
#[derive(Debug)]
struct EchoBroker;

impl Backend for EchoBroker {
    fn broker(&self, tool: &str, args: Value, caller: &Caller) -> Result<Outcome, Unavailable> {
        Ok(Outcome::Ok(json!({
            "ran": "broker",
            "tool": tool,
            "args": args,
            "protocol_version": caller.protocol_version,
            "client": caller.client_name,
        })))
    }

    fn local(&self, tool: &str, args: Value) -> Result<Value, CallError> {
        Ok(json!({ "ran": "front", "tool": tool, "args": args }))
    }
}

#[derive(Debug, Clone, Default)]
struct Client;

impl ClientHandler for Client {}

/// The server's side of the stdio loop, over an in-memory pipe.
async fn serve(io: DuplexStream, mut server: Server<Front<EchoBroker>>) {
    let (read, mut write) = tokio::io::split(io);
    let mut lines = BufReader::new(read).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if let Some(response) = server.handle(&line) {
            let written = async {
                write.write_all(response.as_bytes()).await?;
                write.write_all(b"\n").await?;
                write.flush().await
            };
            if written.await.is_err() {
                return;
            }
        }
    }
}

async fn connect(policy: Policy, mode: ClientLifecycleMode) -> RunningService<RoleClient, Client> {
    let (server_io, client_io) = tokio::io::duplex(1 << 16);
    let front = Front::new(Seen::Policy(policy), None, EchoBroker);
    tokio::spawn(serve(server_io, Server::new(INFO, front)));
    Client
        .serve_with_lifecycle(client_io, mode)
        .await
        .expect("the client connects")
}

fn modern() -> ClientLifecycleMode {
    ClientLifecycleMode::Discover {
        preferred_versions: vec![ProtocolVersion::V_2026_07_28],
    }
}

/// Arguments that fit a tool's input schema: every required property, plus a target and a
/// limit where the tool takes them.
fn arguments(schema: &Value) -> Map<String, Value> {
    let mut args = Map::new();
    let properties = schema["properties"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    let required: Vec<&str> = schema["required"]
        .as_array()
        .map(|r| r.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    for name in properties.keys() {
        let value = match name.as_str() {
            "plan_id" => json!("pl_conformance"),
            "fingerprint" => json!("sha256:00"),
            "reason" => json!("conformance test"),
            "job_id" => json!("01JBCDEFGHJKMNPQRSTVWXYZ01"),
            "code" => json!("0x800f081f"),
            "target" => json!("online"),
            "limit" => json!(5),
            _ if required.contains(&name.as_str()) => panic!("no sample for required {name}"),
            _ => continue,
        };
        args.insert(name.clone(), value);
    }
    args
}

/// Lists the catalog and calls every tool; returns the names in list order.
async fn every_tool(client: &RunningService<RoleClient, Client>, version: &str) -> Vec<String> {
    let listed = client.list_all_tools().await.expect("tools/list");
    let expected = catalog::enabled(&Toolset::DEFAULT);
    let names: Vec<String> = listed.iter().map(|t| t.name.to_string()).collect();
    let want: Vec<&str> = expected.iter().map(|t| t.def.name).collect();
    assert_eq!(names, want, "{version}: list order is the catalog's");
    for (tool, entry) in listed.iter().zip(&expected) {
        let hints = tool.annotations.as_ref().expect("annotations");
        assert_eq!(hints.read_only_hint, Some(entry.def.annotations.read_only));
        assert_eq!(
            hints.destructive_hint,
            Some(entry.def.annotations.destructive)
        );
        let schema = Value::Object((*tool.input_schema).clone());
        let args = arguments(&schema);
        let result = client
            .call_tool(CallToolRequestParams::new(tool.name.clone()).with_arguments(args.clone()))
            .await
            .unwrap_or_else(|e| panic!("{version}: {} failed: {e}", tool.name));
        assert_eq!(result.is_error, Some(false), "{version}: {}", tool.name);
        let out = result.structured_content.expect("structuredContent");
        assert_eq!(out["tool"], tool.name.as_ref());
        assert_eq!(out["args"], Value::Object(args));
        let ran = if entry.runs == catalog::Runs::Front {
            "front"
        } else {
            "broker"
        };
        assert_eq!(out["ran"], ran, "{}", tool.name);
        if ran == "broker" {
            assert_eq!(out["protocol_version"], version, "{}", tool.name);
        }
    }
    names
}

fn apply_everything() -> Policy {
    Policy {
        tier: Tier::ApplyWithApproval,
        ..Policy::default()
    }
}

#[tokio::test]
async fn a_modern_client_discovers_lists_and_calls_every_tool() {
    let client = connect(apply_everything(), modern()).await;
    let info = client.peer_info().expect("discover result");
    assert_eq!(info.protocol_version, ProtocolVersion::V_2026_07_28);
    let server = info.server_info.as_ref().expect("serverInfo in _meta");
    assert_eq!(server.name, "dismpro");
    assert_eq!(info.instructions.as_deref(), Some(INFO.instructions));
    every_tool(&client, MODERN).await;
    client.cancel().await.expect("close");
}

#[tokio::test]
async fn a_legacy_client_initializes_lists_and_calls_every_tool() {
    let client = connect(apply_everything(), ClientLifecycleMode::Initialize).await;
    let info = client.peer_info().expect("initialize result");
    let version = info.protocol_version.to_string();
    assert!(LEGACY.contains(&version.as_str()), "{version}");
    let server = info.server_info.as_ref().expect("serverInfo");
    assert_eq!(server.name, "dismpro");
    assert_eq!(info.instructions.as_deref(), Some(INFO.instructions));
    every_tool(&client, &version).await;
    client.cancel().await.expect("close");
}

#[tokio::test]
async fn refusals_reach_either_client_as_tool_errors() {
    for mode in [modern(), ClientLifecycleMode::Initialize] {
        // The default policy plans but doesn't apply.
        let client = connect(Policy::default(), mode).await;
        let apply = CallToolRequestParams::new("plan_apply").with_arguments(
            json!({ "plan_id": "pl_x", "fingerprint": "sha256:00", "reason": "r" })
                .as_object()
                .cloned()
                .unwrap_or_default(),
        );
        let refused = client.call_tool(apply).await.expect("a tool result");
        assert_eq!(refused.is_error, Some(true));
        let text = refused.content[0].as_text().expect("text").text.clone();
        assert!(text.starts_with("policy_forbids"), "{text}");
        // Bad arguments are a tool error too; an unknown tool is a protocol error.
        let bad = CallToolRequestParams::new("explain_error").with_arguments(
            json!({ "code": 5 })
                .as_object()
                .cloned()
                .unwrap_or_default(),
        );
        assert_eq!(
            client.call_tool(bad).await.expect("result").is_error,
            Some(true)
        );
        let unknown = CallToolRequestParams::new("no_such_tool");
        assert!(client.call_tool(unknown).await.is_err());
        client.cancel().await.expect("close");
    }
}
