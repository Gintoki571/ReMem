//! MCP server over stdio: framing, handshake, tool listing and tool errors.
//! End-to-end tests that need the engine are `#[ignore]` until phase 2 lands.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

const PROTOCOLS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
const TOOLS: [&str; 8] = [
    "remember",
    "recall",
    "entity",
    "invalidate",
    "forget",
    "merge_entities",
    "overview",
    "stats",
];

fn temp_dir() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("neocortex-mcp-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct Mcp {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Mcp {
    fn start() -> Mcp {
        let db = temp_dir().join("memory.db");
        let mut child = Command::new(env!("CARGO_BIN_EXE_neocortex"))
            .arg("mcp")
            .env("NEOCORTEX_DB", &db)
            .env("NEOCORTEX_EMBEDDER", "hash")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn neocortex mcp");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        Mcp {
            child,
            stdin,
            stdout,
        }
    }

    fn send_raw(&mut self, line: &str) {
        writeln!(self.stdin, "{line}").expect("write request");
        self.stdin.flush().expect("flush request");
    }

    fn recv(&mut self) -> Value {
        let mut line = String::new();
        let n = self.stdout.read_line(&mut line).expect("read response");
        assert!(n > 0, "server closed stdout without a response");
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("bad response {line:?}: {e}"))
    }

    fn call(&mut self, msg: Value) -> Value {
        self.send_raw(&msg.to_string());
        self.recv()
    }

    fn initialize(&mut self, version: &str) -> Value {
        self.call(json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": version, "capabilities": {},
                       "clientInfo": {"name": "test", "version": "0"}}
        }))
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn initialize_handshake_and_version_negotiation() {
    let mut mcp = Mcp::start();
    let resp = mcp.initialize("2024-11-05");
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 1);
    let r = &resp["result"];
    assert_eq!(r["protocolVersion"], "2024-11-05");
    assert!(r["capabilities"]["tools"].is_object());
    assert_eq!(r["serverInfo"]["name"], "neocortex");
    assert_eq!(r["serverInfo"]["version"], env!("CARGO_PKG_VERSION"));
    assert!(r["instructions"].as_str().unwrap().len() > 40);

    // A version we do not know falls back to the newest one we support.
    let resp = mcp.initialize("1999-01-01");
    assert_eq!(resp["result"]["protocolVersion"], PROTOCOLS[0]);
    // The newest supported version is echoed verbatim.
    let resp = mcp.initialize(PROTOCOLS[0]);
    assert_eq!(resp["result"]["protocolVersion"], PROTOCOLS[0]);
}

#[test]
fn notifications_get_no_reply() {
    let mut mcp = Mcp::start();
    mcp.send_raw(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
    mcp.send_raw(
        r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":9}}"#,
    );
    let resp = mcp.call(json!({"jsonrpc": "2.0", "id": 7, "method": "ping"}));
    assert_eq!(resp["id"], 7, "notification produced a reply: {resp}");
    assert!(resp.get("result").is_some());
}

#[test]
fn tools_list_returns_the_eight_tools() {
    let mut mcp = Mcp::start();
    mcp.initialize(PROTOCOLS[0]);
    let resp = mcp.call(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}));
    let tools = resp["result"]["tools"].as_array().expect("tools array");
    assert_eq!(tools.len(), TOOLS.len());
    for name in TOOLS {
        let tool = tools
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("tool {name} missing"));
        assert_eq!(tool["inputSchema"]["type"], "object", "{name} schema");
        assert!(tool["description"].as_str().unwrap().len() > 20);
    }
    let remember = tools.iter().find(|t| t["name"] == "remember").unwrap();
    assert_eq!(remember["inputSchema"]["required"], json!(["content"]));
    let recall = tools.iter().find(|t| t["name"] == "recall").unwrap();
    assert_eq!(recall["inputSchema"]["required"], json!(["query"]));
}

#[test]
fn unknown_method_is_32601() {
    let mut mcp = Mcp::start();
    let resp = mcp.call(json!({"jsonrpc": "2.0", "id": 3, "method": "nope/nothing"}));
    assert_eq!(resp["error"]["code"], -32601);
    assert_eq!(resp["id"], 3);
}

#[test]
fn malformed_json_is_32700_and_the_loop_survives() {
    let mut mcp = Mcp::start();
    mcp.send_raw("{not json");
    let resp = mcp.recv();
    assert_eq!(resp["error"]["code"], -32700);
    assert!(resp["id"].is_null());
    let resp = mcp.call(json!({"jsonrpc": "2.0", "id": 4, "method": "ping"}));
    assert_eq!(resp["id"], 4);
}

#[test]
fn missing_required_argument_is_a_tool_error() {
    let mut mcp = Mcp::start();
    mcp.initialize(PROTOCOLS[0]);
    let resp = mcp.call(json!({
        "jsonrpc": "2.0", "id": 5, "method": "tools/call",
        "params": {"name": "remember", "arguments": {}}
    }));
    assert!(
        resp.get("error").is_none(),
        "protocol error instead: {resp}"
    );
    assert_eq!(resp["result"]["isError"], true);
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("content"),
        "message should name the field: {text}"
    );

    let resp = mcp.call(json!({
        "jsonrpc": "2.0", "id": 6, "method": "tools/call",
        "params": {"name": "recall", "arguments": {}}
    }));
    assert_eq!(resp["result"]["isError"], true);
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("query"),
        "message should name the field: {text}"
    );
}

#[test]
fn oversized_recall_query_is_rejected_at_the_tool_boundary() {
    let mut mcp = Mcp::start();
    mcp.initialize(PROTOCOLS[0]);
    let query = "x".repeat(1024 * 1024);
    let started = std::time::Instant::now();
    let resp = mcp.call(json!({
        "jsonrpc": "2.0", "id": 8, "method": "tools/call",
        "params": {"name": "recall", "arguments": {"query": query}}
    }));
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
    assert_eq!(resp["result"]["isError"], true);
    assert!(resp["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("query"));
    assert!(resp["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("2048"));
}

#[test]
fn oversized_remember_collection_is_rejected_with_field_and_limit() {
    let mut mcp = Mcp::start();
    mcp.initialize(PROTOCOLS[0]);
    let entities: Vec<Value> = (0..65)
        .map(|i| json!({"name": format!("entity-{i}")}))
        .collect();
    let resp = mcp.call(json!({
        "jsonrpc": "2.0", "id": 9, "method": "tools/call",
        "params": {"name": "remember", "arguments": {
            "content": "bounded input", "entities": entities
        }}
    }));
    assert_eq!(resp["result"]["isError"], true);
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("entities"));
    assert!(text.contains("64"));
}

// ------------------------------------------------------------ phase 2

#[test]
fn remember_then_recall_through_mcp() {
    let mut mcp = Mcp::start();
    mcp.initialize(PROTOCOLS[0]);
    let saved = mcp.call(json!({
        "jsonrpc": "2.0", "id": 10, "method": "tools/call",
        "params": {"name": "remember", "arguments": {
            "content": "Mom moved from Delhi to Pune on 2026-09-14.",
            "kind": "event", "tags": ["family"],
            "entities": [{"name": "Mom", "type": "person"}, {"name": "Pune", "type": "place"}],
            "relations": [{"source": "Mom", "predicate": "lives in", "target": "Pune", "exclusive": true}]
        }}
    }));
    assert_eq!(saved["result"]["isError"], false, "{saved}");
    assert!(saved["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains('m'));

    let found = mcp.call(json!({
        "jsonrpc": "2.0", "id": 11, "method": "tools/call",
        "params": {"name": "recall", "arguments": {"query": "where does Mom live"}}
    }));
    let text = found["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Pune"), "{text}");
}

#[test]
fn entity_invalidate_and_forget_through_mcp() {
    let mut mcp = Mcp::start();
    mcp.initialize(PROTOCOLS[0]);
    mcp.call(json!({
        "jsonrpc": "2.0", "id": 20, "method": "tools/call",
        "params": {"name": "remember", "arguments": {
            "content": "The deploy runs on Fridays.", "kind": "fact",
            "entities": [{"name": "deploy", "type": "procedure"}]
        }}
    }));
    let e = mcp.call(json!({
        "jsonrpc": "2.0", "id": 21, "method": "tools/call",
        "params": {"name": "entity", "arguments": {"name": "deploy"}}
    }));
    assert_eq!(e["result"]["isError"], false, "{e}");

    let inv = mcp.call(json!({
        "jsonrpc": "2.0", "id": 22, "method": "tools/call",
        "params": {"name": "invalidate", "arguments": {"id": "m1", "at": "tomorrow"}}
    }));
    assert_eq!(inv["result"]["isError"], false, "{inv}");

    let gone = mcp.call(json!({
        "jsonrpc": "2.0", "id": 23, "method": "tools/call",
        "params": {"name": "forget", "arguments": {"id": "m1", "hard": true}}
    }));
    assert_eq!(gone["result"]["isError"], false, "{gone}");
}

#[test]
fn stats_reports_counts() {
    let mut mcp = Mcp::start();
    mcp.initialize(PROTOCOLS[0]);
    let resp = mcp.call(json!({
        "jsonrpc": "2.0", "id": 30, "method": "tools/call",
        "params": {"name": "stats", "arguments": {"format": "json"}}
    }));
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    let v: Value = serde_json::from_str(text).expect("json format");
    assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
}
