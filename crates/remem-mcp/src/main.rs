//! remem-mcp: MCP stdio server exposing the ReMem v3 engine.
//!
//! Transport choice: hand-rolled newline-delimited JSON-RPC 2.0 over
//! stdio (serde_json only), NOT the `rmcp` crate. `rmcp` is at major
//! version 3.x with a heavy async dependency tree (tokio, schemars,
//! transport layers); MCP stdio is just JSON-RPC with `initialize`,
//! `tools/list`, `tools/call` shapes, so ~150 lines of sync stdio code
//! gets a green, dependency-free server TODAY. Revisit `rmcp` if we need
//! Streamable HTTP or sampling/progress features.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use remem_embed::Embedder as _;
use remem_recall::stub::StubEmbedder;
use remem_recall::RecallEngine;
use remem_store::Store;
use remem_types::{MemoryItem, MemoryKind, RecallQuery};
use serde_json::{json, Value};

const PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_NAME: &str = "remem-mcp";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

fn db_path() -> PathBuf {
    let raw = std::env::var("REMEM_DB").unwrap_or_else(|_| "~/.remem/remem.db".to_string());
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(raw)
}

/// Adapter: real local embedder behind recall's minimal Embed trait.
/// Same pattern as `remem-recall/src/main.rs`.
struct RealEmbedder(remem_embed::LocalEmbedder);

impl remem_recall::Embed for RealEmbedder {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        self.0.embed(texts)
    }
}

fn engine() -> Result<RecallEngine> {
    let path = db_path();
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).ok();
        }
    }
    let store = Store::open_path(&path).context("open store")?;
    let graph = remem_graph::Graph::open(&path).map_err(|e| anyhow!("open graph: {e}"))?;
    let embed: Box<dyn remem_recall::Embed> = match remem_embed::load() {
        Ok(m) => {
            eprintln!("embedder: {} ({}d)", m.device_name(), m.dims());
            Box::new(RealEmbedder(m))
        }
        Err(e) => {
            eprintln!("embedder: local model unavailable ({e:#}), using stub");
            Box::new(StubEmbedder)
        }
    };
    Ok(RecallEngine::new(store, embed).with_graph(graph))
}

fn tools_list() -> Value {
    json!([
        {"name": "remember", "description": "Store a memory. kind is one of fact|decision|mistake|preference|event|note. Be selective: store durable facts, decisions, mistakes, and preferences that would be useful to recall in 6 months. Do NOT store transient chatter, verbatim logs, greetings/filler, or anything already remembered.",
         "annotations": {"readOnlyHint": false, "destructiveHint": false},
         "inputSchema": {"type": "object",
            "properties": {"kind": {"type": "string"}, "content": {"type": "string"},
                "tags": {"type": "array", "items": {"type": "string"}},
                "agent": {"type": "string"}, "session": {"type": "string"},
                "importance": {"type": "number"}},
            "required": ["kind", "content"]}},
        {"name": "recall", "description": "Ranked recall over stored memories.",
         "annotations": {"readOnlyHint": true, "destructiveHint": false},
         "inputSchema": {"type": "object",
            "properties": {"query": {"type": "string"}, "k": {"type": "integer"},
                "agent": {"type": "string"}, "session": {"type": "string"}},
            "required": ["query"]}},
        {"name": "list", "description": "List stored memories (newest first).",
         "annotations": {"readOnlyHint": true, "destructiveHint": false},
         "inputSchema": {"type": "object",
            "properties": {"limit": {"type": "integer"}}}},
        {"name": "link", "description": "Link two memories in the graph.",
         "annotations": {"readOnlyHint": false, "destructiveHint": false},
         "inputSchema": {"type": "object",
            "properties": {"from": {"type": "string"}, "to": {"type": "string"},
                "rel": {"type": "string"}},
            "required": ["from", "to"]}},
        {"name": "forget", "description": "Delete a memory by id.",
         "annotations": {"readOnlyHint": false, "destructiveHint": true},
         "inputSchema": {"type": "object",
            "properties": {"id": {"type": "string"}},
            "required": ["id"]}},
        {"name": "purge", "description": "Hard-delete a memory by id (removes row, FTS and vector entries).",
         "annotations": {"readOnlyHint": false, "destructiveHint": true},
         "inputSchema": {"type": "object",
            "properties": {"id": {"type": "string"}},
            "required": ["id"]}},
        {"name": "stats", "description": "Database and graph counts.",
         "annotations": {"readOnlyHint": true, "destructiveHint": false},
         "inputSchema": {"type": "object", "properties": {}}},
        {"name": "validate", "description": "Validate store/graph consistency; returns a list of issues (empty means healthy).",
         "annotations": {"readOnlyHint": true, "destructiveHint": false},
         "inputSchema": {"type": "object", "properties": {}}},
    ])
}

fn text_result(text: String) -> Value {
    json!({"content": [{"type": "text", "text": text}]})
}

fn tool_error(msg: String) -> Value {
    json!({"content": [{"type": "text", "text": msg}], "isError": true})
}

fn str_arg(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn call_tool(eng: &RecallEngine, name: &str, args: &Value) -> Value {
    match dispatch(eng, name, args) {
        Ok(v) => text_result(v),
        Err(e) => tool_error(format!("{e:#}")),
    }
}

fn dispatch(eng: &RecallEngine, name: &str, args: &Value) -> Result<String> {
    let args = if args.is_null() { &json!({}) } else { args };
    match name {
        "remember" => {
            let kind_s = str_arg(args, "kind").unwrap_or_default();
            let kind = MemoryKind::parse(&kind_s).ok_or_else(|| {
                anyhow!("unknown kind '{kind_s}' (fact|decision|mistake|preference|event|note)")
            })?;
            let content =
                str_arg(args, "content").ok_or_else(|| anyhow!("remember: missing 'content'"))?;
            let mut item = MemoryItem::new(kind, content);
            if let Some(tags) = args.get("tags").and_then(|v| v.as_array()) {
                item.tags = tags
                    .iter()
                    .filter_map(|t| t.as_str().map(String::from))
                    .collect();
            }
            if let Some(a) = str_arg(args, "agent") {
                item.agent_id = a;
            }
            if let Some(s) = str_arg(args, "session") {
                item.session_id = s;
            }
            if let Some(imp) = args.get("importance").and_then(|v| v.as_f64()) {
                item.importance = imp as f32;
            }
            let id = eng.remember(&item)?;
            Ok(serde_json::to_string(&json!({"id": id}))?)
        }
        "recall" => {
            let query = str_arg(args, "query").unwrap_or_default();
            let k = args.get("k").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
            let q = RecallQuery {
                text: query,
                k,
                agent_id: str_arg(args, "agent"),
                session_id: str_arg(args, "session"),
                ..Default::default()
            };
            let hits = eng.recall(&q)?;
            let v: Vec<Value> = hits
                .iter()
                .map(|h| {
                    json!({"id": h.item.id, "kind": h.item.kind.as_str(),
                        "content": h.item.content, "tags": h.item.tags,
                        "agent": h.item.agent_id, "session": h.item.session_id,
                        "score": h.score, "reasons": h.reasons})
                })
                .collect();
            Ok(serde_json::to_string(&v)?)
        }
        "list" => {
            let mut items = eng.list()?;
            if let Some(limit) = args.get("limit").and_then(|v| v.as_u64()) {
                items.truncate(limit as usize);
            }
            Ok(serde_json::to_string(&items)?)
        }
        "link" => {
            let from = str_arg(args, "from").ok_or_else(|| anyhow!("link: missing 'from'"))?;
            let to = str_arg(args, "to").ok_or_else(|| anyhow!("link: missing 'to'"))?;
            eng.link(&from, &to, str_arg(args, "rel").as_deref())?;
            Ok(serde_json::to_string(&json!({"ok": true}))?)
        }
        "forget" => {
            let id = str_arg(args, "id").ok_or_else(|| anyhow!("forget: missing 'id'"))?;
            let forgotten = eng.store().get(&id).is_some();
            eng.forget(&id)?;
            Ok(serde_json::to_string(&json!({"forgotten": forgotten}))?)
        }
        "purge" => {
            let id = str_arg(args, "id").ok_or_else(|| anyhow!("purge: missing 'id'"))?;
            let purged = eng.store().purge(&id).map_err(|e| anyhow!("purge: {e}"))?;
            if purged {
                if let Some(g) = eng.graph() {
                    g.forget(&id).map_err(|e| anyhow!("graph forget: {e}"))?;
                }
            }
            Ok(serde_json::to_string(&json!({"purged": purged}))?)
        }
        "stats" => Ok(serde_json::to_string(&eng.stats()?)?),
        "validate" => {
            let g = eng
                .graph()
                .ok_or_else(|| anyhow!("engine has no graph open"))?;
            Ok(serde_json::to_string(&json!({"issues": g.validate()}))?)
        }
        _ => Err(anyhow!("unknown tool '{name}'")),
    }
}

/// Read one JSON-RPC message: plain newline-delimited JSON, or
/// `Content-Length: N` framed (some MCP clients send headers).
fn read_message(reader: &mut BufReader<std::io::StdinLock<'_>>) -> Result<Option<Value>> {
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None); // EOF
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
            let n: usize = rest.trim().parse().context("bad Content-Length")?;
            // consume remaining header lines until blank
            loop {
                let mut h = String::new();
                reader.read_line(&mut h)?;
                if h.trim().is_empty() {
                    break;
                }
            }
            let mut buf = vec![0u8; n];
            reader.read_exact(&mut buf)?;
            return Ok(Some(serde_json::from_slice(&buf)?));
        }
        return Ok(Some(serde_json::from_str(trimmed)?));
    }
}

fn handle(eng: &RecallEngine, msg: &Value) -> Option<Value> {
    let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let id = msg.get("id").cloned().unwrap_or(Value::Null);
    let has_id = msg.get("id").is_some();

    // Notifications (no id) get no response.
    if !has_id {
        return None;
    }

    let result = match method {
        "initialize" => json!({"protocolVersion": PROTOCOL_VERSION,
            "capabilities": {"tools": {}},
            "serverInfo": {"name": SERVER_NAME, "version": SERVER_VERSION}}),
        "tools/list" => json!({"tools": tools_list()}),
        "tools/call" => {
            let params = msg.get("params").cloned().unwrap_or(Value::Null);
            let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(Value::Null);
            call_tool(eng, name, &args)
        }
        "ping" => json!({}),
        _ => {
            return Some(json!({"jsonrpc": "2.0", "id": id,
                "error": {"code": -32601, "message": format!("method not found: {method}")}}));
        }
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

fn main() -> Result<()> {
    let eng = engine()?;
    let stdin = std::io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    while let Some(msg) = read_message(&mut reader)? {
        if let Some(resp) = handle(&eng, &msg) {
            writeln!(out, "{}", serde_json::to_string(&resp)?)?;
            out.flush()?;
        }
    }
    Ok(())
}
