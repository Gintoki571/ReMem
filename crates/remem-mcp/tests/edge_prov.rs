//! MCP surface for edge provenance (issue #15): RED-first. Own Client copy
//! so the shared stdio harness stays untouched.

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
    fn spawn(db: &str) -> Self {
        let exe = ["CARGO_BIN_EXE_remem_mcp", "CARGO_BIN_EXE_remem-mcp"]
            .into_iter()
            .filter_map(|k| std::env::var(k).ok())
            .next()
            .unwrap_or_else(|| "target/debug/remem-mcp".to_string());
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

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        writeln!(self.stdin, "{}", serde_json::to_string(&msg).unwrap()).unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        self.reader.read_line(&mut line).unwrap();
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
    let p = std::env::temp_dir().join(format!("remem-mcp-prov-{name}-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p.to_str().unwrap().to_string()
}

fn init(c: &mut Client) {
    c.request(
        "initialize",
        json!({"protocolVersion": "2024-11-05",
        "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}),
    );
}

#[test]
fn link_provenance_flows_to_related_and_central() {
    let mut c = Client::spawn(&temp_db("prov"));
    init(&mut c);
    let a: Value = c.call(
        "remember",
        json!({"kind": "fact", "content": "mcp prov alpha token"}),
    );
    let b: Value = c.call(
        "remember",
        json!({"kind": "fact", "content": "mcp prov beta token"}),
    );
    let ida = a["id"].as_str().unwrap().to_string();
    let idb = b["id"].as_str().unwrap().to_string();
    let _: Value = c.call(
        "link",
        json!({"from": ida, "to": idb, "rel": "SUPERSEDES", "provenance": "correction-chain"}),
    );
    let rel: Value = c.call("related", json!({"id": ida}));
    assert!(
        rel.as_array().unwrap().iter().any(|n| n["id"] == idb
            && n["rel"] == "SUPERSEDES"
            && n["provenance"] == "correction-chain"),
        "{rel:?}"
    );
    let central: Value = c.call("central", json!({}));
    assert!(
        central.as_array().unwrap().iter().any(|n| n["provenance"]
            .as_array()
            .is_some_and(|p| p.contains(&json!("correction-chain")))),
        "{central:?}"
    );
}

#[test]
fn validate_surfaces_suspect_supersedes() {
    let mut c = Client::spawn(&temp_db("validate"));
    init(&mut c);
    let a: Value = c.call(
        "remember",
        json!({"kind": "fact", "content": "mcp postgres tuning token", "tags": ["db"]}),
    );
    let b: Value = c.call(
        "remember",
        json!({"kind": "fact", "content": "mcp rose pruning token", "tags": ["garden"]}),
    );
    let ida = a["id"].as_str().unwrap().to_string();
    let idb = b["id"].as_str().unwrap().to_string();
    let _: Value = c.call("link", json!({"from": ida, "to": idb, "rel": "SUPERSEDES"}));
    let v: Value = c.call("validate", json!({}));
    let issues = v["issues"].as_array().unwrap();
    assert!(
        issues.iter().any(|i| i
            .as_str()
            .is_some_and(|s| s.contains("suspect SUPERSEDES") && s.contains("no shared tags"))),
        "{issues:?}"
    );
}
