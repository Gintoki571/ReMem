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

/// Max bytes accepted in one `Content-Length` frame. Larger frames are
/// rejected with a JSON-RPC error before any body allocation.
const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;
/// Max `recall.k` / `list.limit` accepted from a client; larger values are
/// clamped in `dispatch` before reaching the engine.
const MAX_TOP_K: usize = 1000;

const PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_NAME: &str = "remem-mcp";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

fn db_path() -> Result<PathBuf> {
    let raw: std::ffi::OsString =
        std::env::var_os("REMEM_DB").unwrap_or_else(|| "~/.remem/remem.db".into());
    let bytes = raw.as_encoded_bytes();
    if let Some(rest) = bytes.strip_prefix(b"~/") {
        let home = std::env::var_os("HOME").filter(|h| !h.is_empty()).ok_or_else(|| {
            anyhow!("REMEM_DB uses ~/ but HOME is unset or empty; set HOME or REMEM_DB to an absolute path")
        })?;
        let mut p = PathBuf::from(home);
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            p.push(std::ffi::OsStr::from_bytes(rest));
        }
        #[cfg(not(unix))]
        {
            p.push(String::from_utf8_lossy(rest).as_ref());
        }
        return Ok(p);
    }
    Ok(PathBuf::from(raw))
}

/// Create the DB parent dir (0700 for newly created levels on Unix) and
/// propagate failures with context instead of ignoring them.
fn ensure_db_parent(path: &std::path::Path) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    // Nearest pre-existing ancestor: never chmod above this (e.g. /tmp).
    let mut existing = parent;
    while !existing.exists() {
        match existing.parent() {
            Some(p) if !p.as_os_str().is_empty() => existing = p,
            _ => break,
        }
    }
    let existing = existing.to_path_buf();
    std::fs::create_dir_all(parent)
        .with_context(|| format!("create db parent dir {}", parent.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut chain = Vec::new();
        let mut d = parent;
        while d != existing && !d.as_os_str().is_empty() {
            chain.push(d.to_path_buf());
            match d.parent() {
                Some(p) if !p.as_os_str().is_empty() => d = p,
                _ => break,
            }
        }
        for dir in chain {
            let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
        }
    }
    Ok(())
}

/// Best-effort 0600 on the DB file (plus WAL sidecars) after open. Unix only.
fn harden_db_file(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut targets = vec![path.to_path_buf()];
        // SQLite WAL sidecars inherit the umask, not the DB mode.
        if let Some(s) = path.to_str() {
            targets.push(PathBuf::from(format!("{s}-wal")));
            targets.push(PathBuf::from(format!("{s}-shm")));
        }
        for t in targets {
            if t.is_file() {
                let _ = std::fs::set_permissions(&t, std::fs::Permissions::from_mode(0o600));
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
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
    let path = db_path()?;
    ensure_db_parent(&path)?;
    let store = Store::open_path(&path).context("open store")?;
    let graph = remem_graph::Graph::open(&path).map_err(|e| anyhow!("open graph: {e}"))?;
    harden_db_file(&path);
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
                "agent": {"type": "string"}, "session": {"type": "string"},
                "maxChars": {"type": "integer"}, "minScore": {"type": "number"}},
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
        {"name": "related", "description": "Graph neighbors of a memory id as [{id, rel}]. Optional rel filters by edge type.",
         "annotations": {"readOnlyHint": true, "destructiveHint": false},
         "inputSchema": {"type": "object",
            "properties": {"id": {"type": "string"}, "rel": {"type": "string"}},
            "required": ["id"]}},
        {"name": "central", "description": "Top central memories by graph PageRank as [{id, score}]. Optional limit (default 10).",
         "annotations": {"readOnlyHint": true, "destructiveHint": false},
         "inputSchema": {"type": "object",
            "properties": {"limit": {"type": "integer"}}}},
        {"name": "path", "description": "Shortest memory-to-memory path from id to id as [from, .., to] (empty if unreachable).",
         "annotations": {"readOnlyHint": true, "destructiveHint": false},
         "inputSchema": {"type": "object",
            "properties": {"from": {"type": "string"}, "to": {"type": "string"}},
            "required": ["from", "to"]}},
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
            match args.get("importance") {
                None | Some(Value::Null) => {}
                Some(v) => match v.as_f64() {
                    Some(imp) => item.importance = imp as f32,
                    None => return Err(anyhow!("remember: 'importance' must be a number")),
                },
            }
            let id = eng.remember(&item)?;
            Ok(serde_json::to_string(&json!({"id": id}))?)
        }
        "recall" => {
            let query = match str_arg(args, "query") {
                Some(q) if !q.is_empty() => q,
                _ => return Err(anyhow!("recall: missing 'query'")),
            };
            let k = match args.get("k") {
                None | Some(Value::Null) => 5,
                Some(v) => match v.as_u64() {
                    Some(n) => (n as usize).min(MAX_TOP_K),
                    None => return Err(anyhow!("recall: 'k' must be a non-negative integer")),
                },
            };
            let min_score = match args.get("minScore") {
                None | Some(Value::Null) => None,
                Some(v) => Some(
                    v.as_f64()
                        .ok_or_else(|| anyhow!("recall: 'minScore' must be a number"))?,
                ),
            };
            let q = RecallQuery {
                text: query,
                k,
                agent_id: str_arg(args, "agent"),
                session_id: str_arg(args, "session"),
                max_chars: args
                    .get("maxChars")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize),
                ..Default::default()
            };
            // Floor lives on the engine (same pattern as the CLI's
            // `--min-score`): a fresh engine on the same DB applies it.
            // Unfiltered calls reuse the shared engine, no reopen.
            let hits = match min_score {
                None => eng.recall(&q)?,
                Some(ms) => engine()?.with_min_score(ms).recall(&q)?,
            };
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
                items.truncate((limit as usize).min(MAX_TOP_K));
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
            let forgotten = eng.store().get(&id).map_err(|e| anyhow!("forget: {e}"))?.is_some();
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
        "related" => {
            // Unknown id stays []: neighbors of nothing is empty, not an error
            // (unlike link, which must resolve both endpoints to create an edge).
            let id = str_arg(args, "id").ok_or_else(|| anyhow!("related: missing 'id'"))?;
            let g = eng
                .graph()
                .ok_or_else(|| anyhow!("engine has no graph open"))?;
            let rel_filter = str_arg(args, "rel").unwrap_or_default();
            let mut out: Vec<Value> = g
                .neighbors(&id)
                .map_err(|e| anyhow!("graph neighbors: {e}"))?
                .into_iter()
                .filter(|(_, rel)| rel_filter.is_empty() || rel == &rel_filter)
                .map(|(nid, rel)| json!({"id": nid, "rel": rel}))
                .collect();
            out.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
            Ok(serde_json::to_string(&out)?)
        }
        "central" => {
            let limit = match args.get("limit") {
                None | Some(Value::Null) => 10,
                Some(v) => match v.as_u64() {
                    Some(n) => (n as usize).min(MAX_TOP_K),
                    None => return Err(anyhow!("central: 'limit' must be a non-negative integer")),
                },
            };
            let g = eng
                .graph()
                .ok_or_else(|| anyhow!("engine has no graph open"))?;
            let ranked = g.central().map_err(|e| anyhow!("graph central: {e}"))?;
            let out: Vec<Value> = ranked
                .into_iter()
                .take(limit)
                .map(|(id, score)| json!({"id": id, "score": score}))
                .collect();
            Ok(serde_json::to_string(&out)?)
        }
        "path" => {
            let from = str_arg(args, "from").ok_or_else(|| anyhow!("path: missing 'from'"))?;
            let to = str_arg(args, "to").ok_or_else(|| anyhow!("path: missing 'to'"))?;
            let g = eng
                .graph()
                .ok_or_else(|| anyhow!("engine has no graph open"))?;
            let path = g
                .shortest_path(&from, &to)
                .map_err(|e| anyhow!("graph path: {e}"))?;
            Ok(serde_json::to_string(&path)?)
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

/// One framed read: either a parsed message or a per-message failure the
/// caller must answer with `-32700` and then keep reading.
enum Frame {
    Msg(Value),
    ParseError(String),
}

/// Read one JSON-RPC message: plain newline-delimited JSON, or
/// `Content-Length: N` framed (some MCP clients send headers).
/// Per-message failures (bad `Content-Length`, oversize frame, short read,
/// invalid JSON) come back as `Frame::ParseError`, never `Err`: only clean
/// EOF (`Ok(None)`) or a fatal stdin failure (`Err`) ends the loop.
fn read_message(reader: &mut BufReader<std::io::StdinLock<'_>>) -> Result<Option<Frame>> {
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
            let n: usize = match rest.trim().parse() {
                Ok(n) => n,
                Err(_) => {
                    return Ok(Some(Frame::ParseError("bad Content-Length".to_string())));
                }
            };
            // Reject before allocating or consuming the body, so a lying
            // header leaves the next message framed and readable.
            if n > MAX_BODY_BYTES {
                return Ok(Some(Frame::ParseError(format!(
                    "Content-Length {n} exceeds {MAX_BODY_BYTES} byte cap"
                ))));
            }
            // consume remaining header lines until blank
            loop {
                let mut h = String::new();
                reader.read_line(&mut h)?;
                if h.trim().is_empty() {
                    break;
                }
            }
            let mut buf = vec![0u8; n];
            if let Err(e) = reader.read_exact(&mut buf) {
                return Ok(Some(Frame::ParseError(format!("short read: {e}"))));
            }
            match serde_json::from_slice(&buf) {
                Ok(v) => return Ok(Some(Frame::Msg(v))),
                Err(e) => return Ok(Some(Frame::ParseError(format!("invalid JSON: {e}")))),
            }
        }
        match serde_json::from_str(trimmed) {
            Ok(v) => return Ok(Some(Frame::Msg(v))),
            Err(e) => return Ok(Some(Frame::ParseError(format!("invalid JSON: {e}")))),
        }
    }
}

fn handle(eng: &RecallEngine, msg: &Value) -> Option<Value> {
    // Valid JSON that is not an object (42, [], null, "str") is an invalid
    // request, not a notification: answer -32600 instead of silent drop.
    if !msg.is_object() {
        return Some(json!({"jsonrpc": "2.0", "id": null,
            "error": {"code": -32600, "message": "invalid request: expected object"}}));
    }
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
    loop {
        match read_message(&mut reader)? {
            None => break,
            Some(Frame::Msg(msg)) => {
                if let Some(resp) = handle(&eng, &msg) {
                    writeln!(out, "{}", serde_json::to_string(&resp)?)?;
                    out.flush()?;
                }
            }
            Some(Frame::ParseError(message)) => {
                let resp = json!({"jsonrpc": "2.0", "id": null,
                    "error": {"code": -32700, "message": message}});
                writeln!(out, "{}", serde_json::to_string(&resp)?)?;
                out.flush()?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static L: OnceLock<Mutex<()>> = OnceLock::new();
        L.get_or_init(|| Mutex::new(()))
    }

    struct EnvGuard {
        _g: std::sync::MutexGuard<'static, ()>,
        old_db: Option<std::ffi::OsString>,
        old_home: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn lock() -> Self {
            let g = env_lock().lock().unwrap();
            Self {
                _g: g,
                old_db: std::env::var_os("REMEM_DB"),
                old_home: std::env::var_os("HOME"),
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.old_db {
                Some(v) => std::env::set_var("REMEM_DB", v),
                None => std::env::remove_var("REMEM_DB"),
            }
            match &self.old_home {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn fresh_db_file_is_0600_and_parent_0700() {
        use std::os::unix::fs::PermissionsExt;
        let _env = EnvGuard::lock();
        let dir = std::env::temp_dir().join(format!(
            "remem-mcp-perm-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let db = dir.join("sub").join("remem.db");
        std::env::set_var("REMEM_DB", &db);
        let _ = std::fs::remove_dir_all(&dir);
        engine().expect("engine opens fresh db");
        let mode = std::fs::metadata(&db).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "db file mode was {mode:o}");
        let pmode = std::fs::metadata(db.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(pmode, 0o700, "db parent mode was {pmode:o}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_remem_db_is_preserved() {
        use std::os::unix::ffi::OsStrExt;
        let _env = EnvGuard::lock();
        let raw = std::ffi::OsStr::from_bytes(b"/tmp/remem-nonutf8-\xff.db");
        std::env::set_var("REMEM_DB", raw);
        let p = db_path().expect("db_path keeps non-UTF8");
        assert_eq!(p.as_os_str().as_bytes(), b"/tmp/remem-nonutf8-\xff.db");
        assert!(!p.as_os_str().is_empty());
    }

    #[test]
    fn home_unset_with_tilde_errors_clearly() {
        let _env = EnvGuard::lock();
        std::env::set_var("REMEM_DB", "~/.remem/remem.db");
        std::env::remove_var("HOME");
        let err = format!("{:?}", db_path().unwrap_err());
        assert!(err.contains("HOME"), "error must name HOME: {err}");
        let eng_err = format!("{:?}", engine().err().expect("engine must fail"));
        assert!(eng_err.contains("HOME"), "engine must propagate: {eng_err}");
    }

    #[test]
    fn create_dir_errors_propagate_with_context() {
        let _env = EnvGuard::lock();
        let dir = std::env::temp_dir().join(format!(
            "remem-mcp-blocker-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let blocker = dir.join("blocker");
        std::fs::write(&blocker, b"x").unwrap();
        let db = blocker.join("remem.db");
        std::env::set_var("REMEM_DB", &db);
        let err = format!("{:#}", engine().err().expect("engine must fail"));
        assert!(
            err.contains("create db parent dir"),
            "must carry context: {err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tilde_expands_under_home() {
        let _env = EnvGuard::lock();
        let home = std::env::temp_dir().join(format!(
            "remem-mcp-home-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&home).unwrap();
        std::env::set_var("HOME", &home);
        std::env::set_var("REMEM_DB", "~/.remem/remem.db");
        let p = db_path().expect("tilde expands");
        assert_eq!(p, home.join(".remem/remem.db"));
        assert!(!p.to_string_lossy().starts_with('~'));
        let _ = std::fs::remove_dir_all(&home);
    }
}
