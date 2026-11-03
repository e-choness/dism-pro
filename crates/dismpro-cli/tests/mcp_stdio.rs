//! `dismpro-cli mcp` over real stdin/stdout (TD-0013 §12).
//!
//! - What the front answers by itself (protocol, catalog, pure and refused calls), with no broker
//!   and no UAC prompt.
//! - Elevated only: the front starting the broker and every broker tool, end to end, with each
//!   call's latency printed for TD-0013 §7. On a runner the elevation needs no prompt.
//!
//! The CLI imports `dismapi.dll`, which Wine doesn't have, so under T2 these tests skip; T3 runs
//! them on Windows with `DISMPRO_CLI` naming the release binary.

#![cfg(windows)]
#![expect(clippy::unwrap_used, reason = "test helpers: any error fails the test")]

use std::{
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    thread::{self, JoinHandle},
    time::Instant,
};

use serde_json::{Value, json};

/// One `dismpro-cli mcp` process, asked one request at a time.
struct Mcp {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    stderr: Option<JoinHandle<String>>,
    next_id: u64,
}

impl Mcp {
    /// `None` when this system can't run the CLI (no DISM, as under Wine).
    fn start(args: &[&str]) -> Option<Self> {
        let root =
            std::env::var_os("SystemRoot").map_or_else(|| "C:\\Windows".into(), PathBuf::from);
        if !root.join("System32").join("dismapi.dll").exists() {
            eprintln!("skipped: no dismapi.dll on this system (Wine); T3 runs this test");
            return None;
        }
        let exe = std::env::var_os("DISMPRO_CLI").map_or_else(
            || PathBuf::from(env!("CARGO_BIN_EXE_dismpro-cli")),
            PathBuf::from,
        );
        let mut child = Command::new(exe)
            .arg("mcp")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stderr = child.stderr.take().unwrap();
        let stderr = thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            text
        });
        Some(Self {
            stdin: child.stdin.take(),
            stdout: BufReader::new(child.stdout.take().unwrap()),
            stderr: Some(stderr),
            child,
            next_id: 1,
        })
    }

    fn send(&mut self, message: &Value) {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{message}").unwrap();
        stdin.flush().unwrap();
    }

    /// Sends a request and reads its response.
    fn ask(&mut self, method: &str, mut params: Value) -> Value {
        params["_meta"] = json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientCapabilities": {},
            "io.modelcontextprotocol/clientInfo": { "name": "mcp-stdio-test", "version": "1" },
        });
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let response = self.read();
        assert_eq!(response["id"], id, "{response}");
        response
    }

    fn read(&mut self) -> Value {
        let mut line = String::new();
        let n = self.stdout.read_line(&mut line).unwrap();
        assert!(n > 0, "the server closed stdout: {}", self.finish_failed());
        serde_json::from_str(&line).unwrap()
    }

    /// A tool's result, which must not be an error.
    fn call(&mut self, name: &str, arguments: &Value) -> Value {
        let r = self.ask(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        );
        assert_eq!(r["result"]["isError"], false, "{name}: {r}");
        r["result"]["structuredContent"].clone()
    }

    /// A tool error's text.
    fn refused(&mut self, name: &str, arguments: &Value) -> String {
        let r = self.ask(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        );
        assert_eq!(r["result"]["isError"], true, "{name}: {r}");
        r["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    /// Closes stdin, which ends the server, and checks it exited cleanly.
    fn finish(mut self) {
        drop(self.stdin.take());
        let status = self.child.wait().unwrap();
        let errors = self.stderr.take().unwrap().join().unwrap();
        assert!(status.success(), "{status}: {errors}");
    }

    fn finish_failed(&mut self) -> String {
        drop(self.stdin.take());
        let status = self.child.wait().unwrap();
        let errors = self
            .stderr
            .take()
            .map(|h| h.join().unwrap())
            .unwrap_or_default();
        format!("{status}: {errors}")
    }
}

fn names(list: &Value) -> Vec<&str> {
    list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect()
}

#[test]
fn a_legacy_client_initializes_and_lists_the_default_tools() {
    let Some(mut mcp) = Mcp::start(&[]) else {
        return;
    };
    mcp.send(
        &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "old-client", "version": "0.9" },
        }}),
    );
    let init = mcp.read();
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "dismpro");
    // A notification gets no answer: the next line read is the list's.
    mcp.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
    mcp.send(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} }));
    let list = mcp.read();
    assert_eq!(list["id"], 2);
    let tools = names(&list);
    for name in [
        "targets_list",
        "system_overview",
        "health_check",
        "plan_apply",
    ] {
        assert!(tools.contains(&name), "{name} in {tools:?}");
    }
    mcp.finish();
}

#[test]
fn toolsets_narrow_the_list() {
    let Some(mut mcp) = Mcp::start(&["--toolsets", "maintain"]) else {
        return;
    };
    let list = mcp.ask("tools/list", json!({}));
    assert_eq!(
        names(&list),
        [
            "health_check",
            "health_diagnose",
            "health_repair_plan",
            "updates_list"
        ]
    );
    mcp.finish();
}

#[test]
fn the_front_answers_pure_and_refused_calls_itself() {
    let Some(mut mcp) = Mcp::start(&[]) else {
        return;
    };
    let explained = mcp.call("explain_error", &json!({ "code": "0x800f081f" }));
    assert_eq!(explained["diagnosis"], "health-finding-source-missing");
    // Bad arguments and unknown tools never reach the broker.
    mcp.refused("explain_error", &json!({ "code": "nonsense" }));
    mcp.refused("explain_error", &json!({ "code": "5", "extra": true }));
    let unknown = mcp.ask(
        "tools/call",
        json!({ "name": "no_such_tool", "arguments": {} }),
    );
    assert!(unknown["error"]["code"].is_i64(), "{unknown}");
    mcp.finish();
}

/// Every broker tool through a real front, broker and DISM. Needs elevation (a runner has it
/// without a prompt); the broker it starts stays up for 15 minutes, which ends with the VM.
#[test]
fn front_and_broker_end_to_end() {
    if !dpro_win::privilege::is_elevated().unwrap() {
        eprintln!("skipped: needs an elevated session (T3 runners have one)");
        return;
    }
    let Some(mut mcp) = Mcp::start(&[]) else {
        return;
    };
    let timed = |mcp: &mut Mcp, name: &str, args: Value| {
        let t = Instant::now();
        let v = mcp.call(name, &args);
        eprintln!(
            "latency {name}: {:.1} ms",
            t.elapsed().as_secs_f64() * 1000.0
        );
        v
    };
    // The first call starts the broker; the rest reuse its connection.
    let targets = timed(&mut mcp, "targets_list", json!({}));
    assert_eq!(targets["targets"][0]["selector"], "online", "{targets}");
    assert_eq!(targets["targets"][0]["allowed"], true);
    let targets = timed(&mut mcp, "targets_list", json!({}));
    assert!(targets["targets"].is_array());

    let overview = timed(&mut mcp, "system_overview", json!({}));
    assert!(
        overview["build"].as_str().unwrap().starts_with("10.0."),
        "{overview}"
    );
    assert!(overview["space"]["total_bytes"].as_u64().unwrap() > 0);
    let health = overview["health"].as_str().unwrap();
    assert!(["healthy", "repairable", "non-repairable"].contains(&health));

    let updates = timed(&mut mcp, "updates_list", json!({ "limit": 5 }));
    assert!(updates["items"].as_array().unwrap().len() <= 5);
    assert!(updates["matched"].as_u64().unwrap() > 0, "{updates}");
    let updates = timed(&mut mcp, "updates_list", json!({ "limit": 5 }));
    assert!(updates["matched"].as_u64().unwrap() > 0);

    let journal = timed(&mut mcp, "journal_list", json!({}));
    assert!(journal["items"].is_array(), "{journal}");
    let diagnosed = timed(&mut mcp, "health_diagnose", json!({ "limit": 3 }));
    assert!(
        diagnosed["items"].as_array().unwrap().len() <= 3,
        "{diagnosed}"
    );
    let checked = timed(&mut mcp, "health_check", json!({}));
    assert_eq!(checked["health"], health);

    let plan = timed(
        &mut mcp,
        "health_repair_plan",
        json!({ "limit_access": true }),
    );
    let id = plan["plan_id"].as_str().unwrap().to_owned();
    assert!(id.starts_with("pl_"), "{plan}");
    let fingerprint = plan["fingerprint"].as_str().unwrap().to_owned();
    assert!(fingerprint.starts_with("sha256:"));
    assert_eq!(plan["summary"]["changes"], 1);
    assert_eq!(plan["summary"]["contacts_windows_update"], false);
    let got = timed(&mut mcp, "plan_get", json!({ "plan_id": id }));
    assert_eq!(got["fingerprint"], fingerprint.as_str());
    assert_eq!(got["items"].as_array().unwrap().len(), 1);

    // The default policy (tier plan) refuses applying; unknown plans say so.
    let apply = json!({ "plan_id": id, "fingerprint": fingerprint, "reason": "test" });
    assert!(
        mcp.refused("plan_apply", &apply)
            .starts_with("policy_forbids")
    );
    let unknown = mcp.refused("plan_get", &json!({ "plan_id": "pl_nope" }));
    assert!(unknown.starts_with("plan_unknown"), "{unknown}");
    let target = mcp.refused("system_overview", &json!({ "target": "Q:" }));
    assert!(target.starts_with("policy_forbids"), "{target}");

    // A second front reaches the same broker on another pipe instance.
    let mut second = Mcp::start(&[]).unwrap();
    let t = Instant::now();
    second.call("targets_list", &json!({}));
    eprintln!(
        "latency second front, first call: {:.1} ms",
        t.elapsed().as_secs_f64() * 1000.0
    );
    second.finish();
    mcp.finish();
}

/// TD-0013 §7 front budgets, printed for T3 (advisory on a shared runner): `initialize` answered
/// within 100 ms of spawning, `tools/list` within 5 ms, and an idle front under 15 MiB.
#[test]
fn front_start_up_listing_and_idle_memory() {
    const RUNS: usize = 7;
    let mut init_ms = Vec::with_capacity(RUNS);
    let mut list_ms = Vec::with_capacity(RUNS);
    let mut memory = None;
    for run in 0..RUNS {
        let spawned = Instant::now();
        let Some(mut mcp) = Mcp::start(&[]) else {
            return;
        };
        mcp.send(
            &json!({ "jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "mcp-init", "version": "1" },
            }}),
        );
        assert_eq!(mcp.read()["result"]["protocolVersion"], "2025-11-25");
        init_ms.push(ms(spawned));
        let listed = Instant::now();
        let list = mcp.ask("tools/list", json!({}));
        list_ms.push(ms(listed));
        assert!(names(&list).contains(&"targets_list"));
        if run == RUNS - 1 {
            thread::sleep(std::time::Duration::from_secs(1)); // idle, as a client leaves it
            memory = Some(working_set(mcp.child.id()));
        }
        mcp.finish();
    }
    eprintln!(
        "mcp-init: initialize after spawn median {:.1} ms (min {:.1}, max {:.1}; budget 100); \
         tools/list median {:.2} ms (max {:.2}; budget 5); {RUNS} runs",
        median(&mut init_ms),
        init_ms[0],
        init_ms[RUNS - 1],
        median(&mut list_ms),
        list_ms[RUNS - 1],
    );
    eprintln!(
        "mcp-init: idle front {} (budget 15 MiB working set)",
        memory.unwrap_or_default()
    );
}

fn ms(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1000.0
}

/// Sorts `v` and returns its median.
fn median(v: &mut [f64]) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

/// A process's working set and private bytes, as Windows reports them, in MiB.
fn working_set(pid: u32) -> String {
    let script = format!(
        "$p = Get-Process -Id {pid}; '{{0:N1}} MiB working set, {{1:N1}} MiB private' -f \
         ($p.WorkingSet64 / 1MB), ($p.PrivateMemorySize64 / 1MB)"
    );
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}
