//! Spawn the server over stdio: initialize + tools/list + a
//! remember/recall/link/stats roundtrip on a temp DB.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use serde_json::{json, Value};

struct Client {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    reader: BufReader<std::process::ChildStdout>,
    next_id: u64,
}

impl Client {
    fn server_exe() -> String {
        for key in ["CARGO_BIN_EXE_remem_mcp", "CARGO_BIN_EXE_remem-mcp"] {
            if let Ok(v) = std::env::var(key) {
                return v;
            }
        }
        let manifest = env!("CARGO_MANIFEST_DIR");
        let target = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".to_string());
        // cwd when run via cargo is the workspace root, so prefer that
        for cand in [
            format!("{target}/debug/remem-mcp"),
            format!("{manifest}/../../{target}/debug/remem-mcp"),
        ] {
            if std::path::Path::new(&cand).exists() {
                return cand;
            }
        }
        format!("{target}/debug/remem-mcp")
    }

    fn spawn(db: &str) -> Self {
        let exe = Self::server_exe();
        let mut child = Command::new(exe)
            .env("REMEM_DB", db)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn remem-mcp");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");
        Self {
            child,
            stdin,
            reader: BufReader::new(stdout),
            next_id: 1,
        }
    }

    fn send_raw(&mut self, line: &str) {
        writeln!(self.stdin, "{line}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn read_resp(&mut self) -> Value {
        let mut line = String::new();
        self.reader.read_line(&mut line).unwrap();
        assert!(!line.trim().is_empty(), "empty response from server");
        serde_json::from_str(line.trim()).unwrap()
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        writeln!(self.stdin, "{}", serde_json::to_string(&msg).unwrap()).unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        self.reader.read_line(&mut line).unwrap();
        assert!(!line.trim().is_empty(), "empty response to {method}");
        serde_json::from_str(line.trim()).unwrap()
    }

    fn call(&mut self, name: &str, args: Value) -> Value {
        let resp = self.request("tools/call", json!({"name": name, "arguments": args}));
        let result = resp.get("result").expect("tools/call result");
        assert_eq!(result.get("isError"), None, "tool {name} errored: {result}");
        let text = result["content"][0]["text"].as_str().unwrap();
        serde_json::from_str(text).unwrap()
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.child.kill().ok();
        self.child.wait().ok();
    }
}

fn temp_db(name: &str) -> String {
    let p = std::env::temp_dir().join(format!("remem-mcp-test-{name}-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p.to_str().unwrap().to_string()
}

#[test]
fn initialize_and_list_tools() {
    let mut c = Client::spawn(&temp_db("init"));
    let resp = c.request(
        "initialize",
        json!({"protocolVersion": "2024-11-05",
        "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}),
    );
    assert_eq!(resp["result"]["serverInfo"]["name"], json!("remem-mcp"));
    assert!(resp["result"]["capabilities"]["tools"].is_object());

    let resp = c.request("tools/list", json!({}));
    let tools = resp["result"]["tools"].as_array().unwrap();
    let names: Vec<_> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for want in [
        "remember", "recall", "list", "link", "forget", "purge", "stats", "validate",
    ] {
        assert!(names.contains(&want), "missing tool {want}: {names:?}");
    }
    let remember = tools.iter().find(|t| t["name"] == "remember").unwrap();
    assert!(
        remember["description"]
            .as_str()
            .unwrap()
            .contains("6 months"),
        "remember description missing selectivity rule: {}",
        remember["description"]
    );
    let recall = tools.iter().find(|t| t["name"] == "recall").unwrap();
    assert_eq!(
        recall["annotations"]["readOnlyHint"],
        serde_json::json!(true)
    );
    let forget = tools.iter().find(|t| t["name"] == "forget").unwrap();
    assert_eq!(
        forget["annotations"]["readOnlyHint"],
        serde_json::json!(false)
    );
    assert_eq!(
        forget["annotations"]["destructiveHint"],
        serde_json::json!(true)
    );
    let purge = tools.iter().find(|t| t["name"] == "purge").unwrap();
    assert_eq!(
        purge["annotations"]["readOnlyHint"],
        serde_json::json!(false)
    );
    assert_eq!(
        purge["annotations"]["destructiveHint"],
        serde_json::json!(true)
    );
}

#[test]
fn purge_existing_then_recall_empty_and_unknown_false() {
    let mut c = Client::spawn(&temp_db("purge"));
    c.request(
        "initialize",
        json!({"protocolVersion": "2024-11-05",
        "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}),
    );

    let r: Value = c.call(
        "remember",
        json!({"kind": "fact", "content": "mcp purge zulu unique token"}),
    );
    let id = r["id"].as_str().unwrap().to_string();

    let p: Value = c.call("purge", json!({"id": id}));
    assert_eq!(p["purged"], json!(true), "purge existing: {p:?}");

    let hits: Value = c.call("recall", json!({"query": "mcp purge zulu", "k": 5}));
    let hits = hits.as_array().unwrap();
    assert!(
        hits.iter().all(|h| h["id"] != id),
        "purged id still recalled: {hits:?}"
    );

    let p2: Value = c.call("purge", json!({"id": "does-not-exist"}));
    assert_eq!(p2["purged"], json!(false), "purge unknown: {p2:?}");
}

#[test]
fn remember_recall_link_stats_roundtrip() {
    let mut c = Client::spawn(&temp_db("roundtrip"));
    c.request(
        "initialize",
        json!({"protocolVersion": "2024-11-05",
        "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}),
    );

    let a: Value = c.call(
        "remember",
        json!({"kind": "fact", "content": "mcp roundtrip alpha token", "tags": ["t1"]}),
    );
    let b: Value = c.call(
        "remember",
        json!({"kind": "note", "content": "mcp roundtrip beta token"}),
    );
    let ida = a["id"].as_str().unwrap();
    let idb = b["id"].as_str().unwrap();
    assert!(!ida.is_empty() && !idb.is_empty() && ida != idb);

    let _: Value = c.call("link", json!({"from": ida, "to": idb}));

    let hits: Value = c.call("recall", json!({"query": "mcp roundtrip alpha", "k": 5}));
    let hits = hits.as_array().unwrap();
    assert!(!hits.is_empty(), "expected recall hits");
    assert!(
        hits.iter().any(|h| h["id"] == ida),
        "remembered id missing: {hits:?}"
    );

    let items: Value = c.call("list", json!({"limit": 10}));
    assert!(items.as_array().unwrap().len() >= 2);

    let stats: Value = c.call("stats", json!({}));
    assert!(stats["memories"].as_u64().unwrap() >= 2, "stats: {stats:?}");

    let v: Value = c.call("validate", json!({}));
    assert_eq!(v["issues"], json!([]), "expected healthy db: {v:?}");

    let f: Value = c.call("forget", json!({"id": ida}));
    assert_eq!(f["forgotten"], json!(true), "forget existing: {f:?}");
    let f2: Value = c.call("forget", json!({"id": "does-not-exist"}));
    assert_eq!(f2["forgotten"], json!(false), "forget unknown: {f2:?}");

    // unknown tool surfaces as isError, not a protocol error
    let resp = c.request("tools/call", json!({"name": "nope", "arguments": {}}));
    assert_eq!(resp["result"]["isError"], json!(true));
}

#[test]
fn related_returns_linked_neighbor_and_unknown_empty() {
    let mut c = Client::spawn(&temp_db("related"));
    c.request(
        "initialize",
        json!({"protocolVersion": "2024-11-05",
        "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}),
    );

    let a: Value = c.call(
        "remember",
        json!({"kind": "fact", "content": "mcp related alpha token"}),
    );
    let b: Value = c.call(
        "remember",
        json!({"kind": "fact", "content": "mcp related beta token"}),
    );
    let ida = a["id"].as_str().unwrap().to_string();
    let idb = b["id"].as_str().unwrap().to_string();

    let _: Value = c.call("link", json!({"from": ida, "to": idb, "rel": "refutes"}));

    let rel: Value = c.call("related", json!({"id": ida}));
    let arr = rel.as_array().unwrap();
    assert!(
        arr.iter().any(|n| n["id"] == idb && n["rel"] == "refutes"),
        "expected linked neighbor: {rel:?}"
    );

    let filtered: Value = c.call("related", json!({"id": ida, "rel": "supports"}));
    assert_eq!(filtered, json!([]), "rel filter mismatch: {filtered:?}");

    let unknown: Value = c.call("related", json!({"id": "does-not-exist"}));
    assert_eq!(unknown, json!([]), "unknown id: {unknown:?}");
}

#[test]
fn recall_max_chars_truncates_and_unknown_empty_unchanged() {
    let mut c = Client::spawn(&temp_db("maxchars"));
    c.request(
        "initialize",
        json!({"protocolVersion": "2024-11-05",
        "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}),
    );

    // tools/list exposes the optional maxChars arg.
    let resp = c.request("tools/list", json!({}));
    let tools = resp["result"]["tools"].as_array().unwrap();
    let recall = tools.iter().find(|t| t["name"] == "recall").unwrap();
    assert!(
        recall["inputSchema"]["properties"]
            .get("maxChars")
            .is_some(),
        "recall schema missing maxChars: {recall:?}"
    );

    let long_content = format!("mcpbudget entry long {}", "z".repeat(200));
    let _: Value = c.call("remember", json!({"kind": "fact", "content": long_content}));
    let _: Value = c.call(
        "remember",
        json!({"kind": "fact", "content": "mcpbudget entry short a"}),
    );
    let _: Value = c.call(
        "remember",
        json!({"kind": "fact", "content": "mcpbudget entry short b"}),
    );

    let full: Value = c.call("recall", json!({"query": "mcpbudget entry", "k": 5}));
    let full_hits = full.as_array().unwrap();
    assert!(full_hits.len() >= 2, "expected multiple hits: {full:?}");
    let full_chars: usize = full_hits
        .iter()
        .map(|h| h["content"].as_str().unwrap_or("").len())
        .sum();

    let budgeted: Value = c.call(
        "recall",
        json!({"query": "mcpbudget entry", "k": 5, "maxChars": 80}),
    );
    let budgeted_hits = budgeted.as_array().unwrap();
    assert!(
        !budgeted_hits.is_empty(),
        "budget must keep at least one hit"
    );
    let budgeted_chars: usize = budgeted_hits
        .iter()
        .map(|h| h["content"].as_str().unwrap_or("").len())
        .sum();
    assert!(
        budgeted_chars < full_chars,
        "budgeted ({budgeted_chars}) must be shorter than full ({full_chars})"
    );
    assert!(
        budgeted_hits.len() <= full_hits.len(),
        "budgeted hits must not exceed full hits: {budgeted:?} vs {full:?}"
    );

    // Unknown-id / empty behaviors unchanged (fresh DB recalls empty).
    let mut e = Client::spawn(&temp_db("maxchars-empty"));
    e.request(
        "initialize",
        json!({"protocolVersion": "2024-11-05",
        "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}),
    );
    let empty: Value = e.call("recall", json!({"query": "mcpbudget entry", "k": 5}));
    assert_eq!(empty, json!([]), "empty recall: {empty:?}");
    let unknown: Value = c.call("related", json!({"id": "does-not-exist"}));
    assert_eq!(unknown, json!([]), "unknown related: {unknown:?}");
    let f: Value = c.call("forget", json!({"id": "does-not-exist"}));
    assert_eq!(f["forgotten"], json!(false), "forget unknown: {f:?}");
}

#[test]
fn parse_error_replies_and_continues_loop() {
    let mut c = Client::spawn(&temp_db("parse-err"));
    c.send_raw("this is not json{{{");
    let err = c.read_resp();
    assert_eq!(err["error"]["code"], json!(-32700), "garbage: {err:?}");
    assert_eq!(err["id"], json!(null), "garbage: {err:?}");

    c.send_raw("Content-Length: not-a-number");
    let err2 = c.read_resp();
    assert_eq!(err2["error"]["code"], json!(-32700), "bad header: {err2:?}");

    let resp = c.request(
        "initialize",
        json!({"protocolVersion": "2024-11-05",
        "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}),
    );
    assert_eq!(resp["result"]["serverInfo"]["name"], json!("remem-mcp"));
}

#[test]
fn content_length_cap_rejects_huge_frame() {
    let mut c = Client::spawn(&temp_db("cap"));
    c.send_raw("Content-Length: 1073741824");
    c.send_raw("");
    c.send_raw(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#);
    let err = c.read_resp();
    assert!(
        err.get("error").is_some(),
        "expected JSON-RPC error: {err:?}"
    );
    let ok = c.read_resp();
    assert_eq!(ok["result"]["serverInfo"]["name"], json!("remem-mcp"));
    assert_eq!(ok["id"], json!(1));
}

#[test]
fn recall_k_and_list_limit_clamped_to_1000() {
    let mut c = Client::spawn(&temp_db("clamp"));
    c.request(
        "initialize",
        json!({"protocolVersion": "2024-11-05",
        "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}),
    );
    for i in 0..3 {
        let _: Value = c.call(
            "remember",
            json!({"kind": "fact", "content": format!("clamp token entry {i}")}),
        );
    }
    let huge: Value = c.call(
        "recall",
        json!({"query": "clamp token entry", "k": 99999999}),
    );
    let capped: Value = c.call("recall", json!({"query": "clamp token entry", "k": 1000}));
    assert_eq!(
        huge.as_array().unwrap().len(),
        capped.as_array().unwrap().len(),
        "k=99999999 must behave like k=1000: {huge:?}"
    );
    assert!(huge.as_array().unwrap().len() <= 1000);
    let l_huge: Value = c.call("list", json!({"limit": 99999999}));
    let l_cap: Value = c.call("list", json!({"limit": 1000}));
    assert_eq!(
        l_huge.as_array().unwrap().len(),
        l_cap.as_array().unwrap().len(),
        "limit huge must behave like 1000"
    );
    assert!(l_huge.as_array().unwrap().len() <= 1000);
}

#[test]
fn non_object_json_gets_invalid_request() {
    let mut c = Client::spawn(&temp_db("non-object"));
    for raw in ["42", "[1,2,3]", "null", "\"hello\""] {
        c.send_raw(raw);
        let err = c.read_resp();
        assert_eq!(err["error"]["code"], json!(-32600), "input {raw}: {err:?}");
        assert_eq!(err["id"], json!(null), "input {raw}: {err:?}");
    }
    let resp = c.request("ping", json!({}));
    assert_eq!(resp["result"], json!({}), "server must continue: {resp:?}");
}

#[test]
fn recall_and_remember_coercions_error_loudly() {
    let mut c = Client::spawn(&temp_db("coerce"));
    c.request(
        "initialize",
        json!({"protocolVersion": "2024-11-05",
        "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}),
    );
    for args in [json!({}), json!({"k": 5}), json!({"query": ""})] {
        let resp = c.request(
            "tools/call",
            json!({"name": "recall", "arguments": args.clone()}),
        );
        assert_eq!(
            resp["result"]["isError"],
            json!(true),
            "recall {args}: {resp:?}"
        );
    }
    for args in [
        json!({"query": "q", "k": -5}),
        json!({"query": "q", "k": "many"}),
        json!({"query": "q", "k": 5.5}),
        json!({"query": "q", "k": true}),
    ] {
        let resp = c.request(
            "tools/call",
            json!({"name": "recall", "arguments": args.clone()}),
        );
        assert_eq!(
            resp["result"]["isError"],
            json!(true),
            "recall {args}: {resp:?}"
        );
    }
    let resp = c.request("tools/call",
        json!({"name": "remember", "arguments": {"kind": "fact", "content": "coerce imp", "importance": "high"}}));
    assert_eq!(
        resp["result"]["isError"],
        json!(true),
        "importance str: {resp:?}"
    );
    let unknown: Value = c.call("related", json!({"id": "does-not-exist"}));
    assert_eq!(unknown, json!([]), "unknown related stays []: {unknown:?}");
}
