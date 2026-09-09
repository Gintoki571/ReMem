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

#[test]
fn recall_min_score_filters_below_floor() {
    let mut c = Client::spawn(&temp_db("minscore"));
    c.request(
        "initialize",
        json!({"protocolVersion": "2024-11-05",
        "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}),
    );

    // tools/list exposes the optional minScore arg.
    let resp = c.request("tools/list", json!({}));
    let tools = resp["result"]["tools"].as_array().unwrap();
    let recall = tools.iter().find(|t| t["name"] == "recall").unwrap();
    assert!(
        recall["inputSchema"]["properties"]
            .get("minScore")
            .is_some(),
        "recall schema missing minScore: {recall:?}"
    );

    let _: Value = c.call(
        "remember",
        json!({"kind": "fact", "content": "minscore floor check delta token"}),
    );

    let hits: Value = c.call(
        "recall",
        json!({"query": "minscore floor check delta", "k": 5}),
    );
    let hits = hits.as_array().unwrap();
    assert!(!hits.is_empty(), "unfiltered recall must return hits");
    assert!(
        hits.iter().all(|h| h["score"].as_f64().unwrap() < 1.0),
        "fixture scores must sit below the 1.0 floor: {hits:?}"
    );

    let floored: Value = c.call(
        "recall",
        json!({"query": "minscore floor check delta", "k": 5, "minScore": 1.0}),
    );
    assert_eq!(
        floored,
        json!([]),
        "minScore 1.0 must floor all hits: {floored:?}"
    );

    // Non-number minScore errors loudly, like k.
    let resp = c.request(
        "tools/call",
        json!({"name": "recall",
            "arguments": {"query": "minscore floor check delta", "minScore": "high"}}),
    );
    assert_eq!(
        resp["result"]["isError"],
        json!(true),
        "minScore str: {resp:?}"
    );
}

#[test]
fn central_returns_scored_mids_on_linked_data() {
    let mut c = Client::spawn(&temp_db("central"));
    c.request(
        "initialize",
        json!({"protocolVersion": "2024-11-05",
        "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}),
    );

    let resp = c.request("tools/list", json!({}));
    let tools = resp["result"]["tools"].as_array().unwrap();
    let central = tools.iter().find(|t| t["name"] == "central").unwrap();
    assert_eq!(central["annotations"]["readOnlyHint"], json!(true));
    let path = tools.iter().find(|t| t["name"] == "path").unwrap();
    assert_eq!(path["annotations"]["readOnlyHint"], json!(true));

    let a: Value = c.call(
        "remember",
        json!({"kind": "fact", "content": "central alpha hub token"}),
    );
    let b: Value = c.call(
        "remember",
        json!({"kind": "fact", "content": "central beta spoke token"}),
    );
    let cc: Value = c.call(
        "remember",
        json!({"kind": "fact", "content": "central gamma spoke token"}),
    );
    let ida = a["id"].as_str().unwrap().to_string();
    let idb = b["id"].as_str().unwrap().to_string();
    let idc = cc["id"].as_str().unwrap().to_string();
    let _: Value = c.call("link", json!({"from": idb, "to": ida}));
    let _: Value = c.call("link", json!({"from": idc, "to": ida}));

    let ranked: Value = c.call("central", json!({}));
    let arr = ranked.as_array().unwrap();
    assert!(
        !arr.is_empty(),
        "central must rank linked memories: {ranked:?}"
    );
    for entry in arr {
        assert!(entry["id"].is_string(), "scored mid: {entry:?}");
        assert!(entry["score"].is_number(), "scored mid: {entry:?}");
    }
    let ids: Vec<_> = arr.iter().filter_map(|e| e["id"].as_str()).collect();
    assert!(ids.contains(&ida.as_str()), "hub missing: {ranked:?}");

    let limited: Value = c.call("central", json!({"limit": 1}));
    assert_eq!(limited.as_array().unwrap().len(), 1, "limit 1: {limited:?}");

    let resp = c.request(
        "tools/call",
        json!({"name": "central", "arguments": {"limit": "many"}}),
    );
    assert_eq!(
        resp["result"]["isError"],
        json!(true),
        "limit str: {resp:?}"
    );
}

#[test]
fn path_chain_and_unknown_empty() {
    let mut c = Client::spawn(&temp_db("path"));
    c.request(
        "initialize",
        json!({"protocolVersion": "2024-11-05",
        "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}),
    );

    let a: Value = c.call(
        "remember",
        json!({"kind": "fact", "content": "path alpha token"}),
    );
    let b: Value = c.call(
        "remember",
        json!({"kind": "fact", "content": "path beta token"}),
    );
    let cc: Value = c.call(
        "remember",
        json!({"kind": "fact", "content": "path gamma token"}),
    );
    let ida = a["id"].as_str().unwrap().to_string();
    let idb = b["id"].as_str().unwrap().to_string();
    let idc = cc["id"].as_str().unwrap().to_string();
    let _: Value = c.call("link", json!({"from": ida, "to": idb}));
    let _: Value = c.call("link", json!({"from": idb, "to": idc}));

    let p: Value = c.call("path", json!({"from": ida, "to": idc}));
    assert_eq!(p, json!([ida, idb, idc]), "A->B->C: {p:?}");

    let unreachable: Value = c.call("path", json!({"from": idc, "to": ida}));
    assert_eq!(
        unreachable,
        json!([]),
        "reverse unreachable: {unreachable:?}"
    );

    let unknown: Value = c.call("path", json!({"from": "does-not-exist", "to": idc}));
    assert_eq!(unknown, json!([]), "unknown from: {unknown:?}");

    for args in [json!({}), json!({"from": ida}), json!({"to": idc})] {
        let resp = c.request(
            "tools/call",
            json!({"name": "path", "arguments": args.clone()}),
        );
        assert_eq!(
            resp["result"]["isError"],
            json!(true),
            "path {args}: {resp:?}"
        );
    }
}

#[test]
fn recall_since_until_bounds_event_time() {
    let mut c = Client::spawn(&temp_db("since-until"));
    c.request(
        "initialize",
        json!({"protocolVersion": "2024-11-05",
        "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}),
    );

    let resp = c.request("tools/list", json!({}));
    let tools = resp["result"]["tools"].as_array().unwrap();
    let recall = tools.iter().find(|t| t["name"] == "recall").unwrap();
    assert!(
        recall["inputSchema"]["properties"].get("since").is_some(),
        "recall schema missing since: {recall:?}"
    );
    assert!(
        recall["inputSchema"]["properties"].get("until").is_some(),
        "recall schema missing until: {recall:?}"
    );

    let r: Value = c.call(
        "remember",
        json!({"kind": "fact", "content": "sinceuntil backdate quartz token",
            "occurredAt": "2020-01-01"}),
    );
    let id = r["id"].as_str().unwrap().to_string();

    let found: Value = c.call(
        "recall",
        json!({"query": "sinceuntil backdate quartz", "k": 5, "until": "2021-01-01"}),
    );
    assert!(
        found.as_array().unwrap().iter().any(|h| h["id"] == id),
        "until-after-it must find backdated hit: {found:?}"
    );

    let hidden: Value = c.call(
        "recall",
        json!({"query": "sinceuntil backdate quartz", "k": 5, "since": "2021-01-01"}),
    );
    assert!(
        hidden.as_array().unwrap().iter().all(|h| h["id"] != id),
        "since-after-it must hide backdated hit: {hidden:?}"
    );

    let resp = c.request(
        "tools/call",
        json!({"name": "recall",
            "arguments": {"query": "sinceuntil backdate quartz", "since": "not-a-date"}}),
    );
    assert_eq!(
        resp["result"]["isError"],
        json!(true),
        "bad since: {resp:?}"
    );
}

/// `remember` returns a `similar` array: near-duplicate neighbours of the new
/// embedding, probed before insert. Distances depend on the embedder (stub vs
/// real BERT differ), so assertions are structural, not numeric.
#[test]
fn remember_returns_similar_array_for_paraphrase() {
    let mut c = Client::spawn(&temp_db("similar"));
    c.request(
        "initialize",
        json!({"protocolVersion": "2024-11-05",
        "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}),
    );

    let base = "the deploy script uses rsync over ssh to copy artifacts to the bastion host";
    let para = "the deploy script uses rsync over ssh to copy artifacts to the bastion host today";
    let other = "banana bread recipe needs three ripe bananas and a handful of walnuts";

    let a: Value = c.call("remember", json!({"kind": "fact", "content": base}));
    assert_eq!(
        a["similar"],
        json!([]),
        "empty store must stay silent: {a:?}"
    );
    let ida = a["id"].as_str().unwrap().to_string();

    let b: Value = c.call("remember", json!({"kind": "fact", "content": para}));
    let sim = b["similar"].as_array().unwrap();
    assert!(!sim.is_empty(), "paraphrase must report a near-dup: {b:?}");
    assert!(sim.len() <= 3, "the probe is capped at 3: {b:?}");
    assert_eq!(sim[0]["id"], json!(ida), "want the stored id: {b:?}");
    assert!(sim[0]["distance"].as_f64().unwrap() > 0.0, "{b:?}");

    let d: Value = c.call("remember", json!({"kind": "fact", "content": other}));
    assert_eq!(
        d["similar"],
        json!([]),
        "distinct memory stays silent: {d:?}"
    );

    // An exact re-save dedups on content_hash and must not double-report.
    let dup: Value = c.call("remember", json!({"kind": "fact", "content": base}));
    assert_eq!(
        dup["id"],
        json!(ida),
        "exact dup returns the stored id: {dup:?}"
    );
    assert_eq!(dup["similar"], json!([]), "no double-report: {dup:?}");
}

#[test]
fn content_length_header_is_case_insensitive() {
    for (tag, header) in [("lower", "content-length"), ("mixed", "CoNtEnT-LeNgTh")] {
        let mut c = Client::spawn(&temp_db(&format!("ci-{tag}")));
        let body = serde_json::to_string(
            &json!({"jsonrpc": "2.0", "id": 99, "method": "ping", "params": {}}),
        )
        .unwrap();
        c.send_raw(&format!("{}: {}", header, body.len()));
        c.send_raw("");
        c.send_raw(&body);
        let resp = c.read_resp();
        assert_eq!(
            resp["result"],
            json!({}),
            "{tag} header framed ping failed: {resp:?}"
        );
        assert_eq!(resp["id"], json!(99), "{tag} header id: {resp:?}");
    }
}
