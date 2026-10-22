//! MCP server over stdio: newline-delimited JSON-RPC 2.0, one response line
//! per request, flushed immediately. Eight tools, all backed by one `Engine`.
//!
//! The embedder is lazy: `initialize` and `tools/list` never touch the model
//! (loading UltraBERT takes seconds and can fail); the first call that needs
//! an embedding reports the load error as a tool error.

use crate::embed::{self, Embedder, Mode};
use crate::engine::{self, Engine, RecallInput, RememberInput};
use crate::error::{Error, Result};
use crate::render;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::OnceLock;

/// Largest accepted request line (16 MiB).
pub const MAX_MESSAGE: usize = 16 * 1024 * 1024;

const PROTOCOLS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "neocortex is a local long-term memory for AI agents, stored in one SQLite file. \
Call recall at the start of a task and before decisions that may depend on past context (people, projects, \
preferences, earlier work). Call remember for durable facts, decisions and their reasons, preferences, \
mistakes and their fixes; keep secrets and transient chatter out of it.";

// ------------------------------------------------------------------ embedder

/// Loads UltraBERT on first use and caches the result (or the error).
pub struct LazyEmbedder {
    dir: Option<PathBuf>,
    loaded: OnceLock<std::result::Result<Box<dyn Embedder>, String>>,
}

impl LazyEmbedder {
    pub fn new() -> LazyEmbedder {
        LazyEmbedder {
            dir: None,
            loaded: OnceLock::new(),
        }
    }

    /// Load from an explicit model dir instead of NEOCORTEX_MODEL_DIR.
    pub fn with_dir(dir: impl Into<PathBuf>) -> LazyEmbedder {
        LazyEmbedder {
            dir: Some(dir.into()),
            loaded: OnceLock::new(),
        }
    }

    fn get(&self) -> Result<&dyn Embedder> {
        let slot = self
            .loaded
            .get_or_init(|| embed::load(self.dir.as_deref()).map_err(|e| e.to_string()));
        match slot {
            Ok(emb) => Ok(emb.as_ref()),
            Err(msg) => Err(Error(msg.clone())),
        }
    }
}

impl Default for LazyEmbedder {
    fn default() -> Self {
        Self::new()
    }
}

impl Embedder for LazyEmbedder {
    fn embed(&self, texts: &[&str], mode: Mode) -> Result<Vec<Vec<f32>>> {
        self.get()?.embed(texts, mode)
    }

    fn name(&self) -> &str {
        "ultrabert-150m"
    }
}

// ------------------------------------------------------------------ server

/// Serve JSON-RPC on stdin/stdout until stdin closes.
pub fn serve(engine: &Engine) -> Result<()> {
    let stdin = std::io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    loop {
        match read_message(&mut reader, MAX_MESSAGE)? {
            Incoming::Eof => break,
            Incoming::TooLarge => respond(
                &mut out,
                &error(Value::Null, -32600, "request exceeds the 16 MiB limit"),
            )?,
            Incoming::Line(line) => {
                if line.iter().all(|b| b.is_ascii_whitespace()) {
                    continue;
                }
                if let Some(reply) = handle_line(engine, &line) {
                    respond(&mut out, &reply)?;
                }
            }
        }
    }
    Ok(())
}

enum Incoming {
    Eof,
    TooLarge,
    Line(Vec<u8>),
}

/// Read one newline-terminated message, rejecting anything over `max` bytes
/// (the rest of that line is drained so the next request still parses).
fn read_message(reader: &mut impl BufRead, max: usize) -> std::io::Result<Incoming> {
    let mut line: Vec<u8> = Vec::new();
    let mut too_large = false;
    loop {
        let (take, done) = {
            let buf = reader.fill_buf()?;
            if buf.is_empty() {
                if line.is_empty() && !too_large {
                    return Ok(Incoming::Eof);
                }
                break;
            }
            match buf.iter().position(|&b| b == b'\n') {
                Some(i) => {
                    if !too_large {
                        if line.len() + i > max {
                            too_large = true;
                            line.clear();
                        } else {
                            line.extend_from_slice(&buf[..i]);
                        }
                    }
                    (i + 1, true)
                }
                None => {
                    if !too_large {
                        if line.len() + buf.len() > max {
                            too_large = true;
                            line.clear();
                        } else {
                            line.extend_from_slice(buf);
                        }
                    }
                    (buf.len(), false)
                }
            }
        };
        reader.consume(take);
        if done {
            break;
        }
    }
    Ok(if too_large {
        Incoming::TooLarge
    } else {
        Incoming::Line(line)
    })
}

fn respond(out: &mut impl Write, value: &Value) -> Result<()> {
    serde_json::to_writer(&mut *out, value)?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}

fn ok(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

#[derive(Debug)]
struct RpcError {
    code: i64,
    message: String,
}

fn rpc<T>(code: i64, message: impl Into<String>) -> std::result::Result<T, RpcError> {
    Err(RpcError {
        code,
        message: message.into(),
    })
}

fn handle_line(engine: &Engine, line: &[u8]) -> Option<Value> {
    let message: Value = match serde_json::from_slice(line) {
        Ok(v) => v,
        Err(e) => return Some(error(Value::Null, -32700, &format!("parse error: {e}"))),
    };
    let obj = match message.as_object() {
        Some(o) => o,
        None => return Some(error(Value::Null, -32600, "request must be an object")),
    };
    let has_id = obj.contains_key("id");
    let id = obj.get("id").cloned().unwrap_or(Value::Null);
    let Some(method) = obj.get("method").and_then(Value::as_str) else {
        // A notification-shaped message without a method is ignored.
        return if has_id {
            Some(error(id, -32600, "missing method"))
        } else {
            None
        };
    };
    // Notifications never get a reply (JSON-RPC: no id, no response).
    if !has_id || method.starts_with("notifications/") {
        return None;
    }
    let params = obj.get("params").cloned().unwrap_or(Value::Null);
    match dispatch(engine, method, &params) {
        Ok(result) => Some(ok(id, result)),
        Err(e) => Some(error(id, e.code, &e.message)),
    }
}

fn dispatch(engine: &Engine, method: &str, params: &Value) -> std::result::Result<Value, RpcError> {
    match method {
        "initialize" => Ok(initialize(params)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tool_defs()})),
        "tools/call" => call_tool(engine, params),
        _ => rpc(-32601, format!("method not found: {method}")),
    }
}

fn initialize(params: &Value) -> Value {
    let requested = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let version = if PROTOCOLS.contains(&requested) {
        requested
    } else {
        PROTOCOLS[0]
    };
    json!({
        "protocolVersion": version,
        "capabilities": {"tools": {}},
        "serverInfo": {"name": "neocortex", "version": env!("CARGO_PKG_VERSION")},
        "instructions": INSTRUCTIONS,
    })
}

// ------------------------------------------------------------------ tools

#[derive(Clone, Copy)]
enum Format {
    Text,
    Json,
}

#[derive(Debug)]
enum ToolError {
    Unknown,
    Failed(String),
}

fn call_tool(engine: &Engine, params: &Value) -> std::result::Result<Value, RpcError> {
    let name = match params.get("name").and_then(Value::as_str) {
        Some(n) => n,
        None => return rpc(-32602, "tools/call needs a tool name"),
    };
    let args = match params.get("arguments") {
        None | Some(Value::Null) => Value::Object(Map::new()),
        Some(v @ Value::Object(_)) => v.clone(),
        Some(_) => return rpc(-32602, "tool arguments must be an object"),
    };
    match run_tool(engine, name, &args) {
        Ok(text) => Ok(json!({
            "content": [{"type": "text", "text": text}],
            "isError": false
        })),
        Err(ToolError::Unknown) => rpc(-32602, format!("unknown tool: {name}")),
        Err(ToolError::Failed(message)) => Ok(json!({
            "content": [{"type": "text", "text": message}],
            "isError": true
        })),
    }
}

fn run_tool(engine: &Engine, name: &str, args: &Value) -> std::result::Result<String, ToolError> {
    let format = match args.get("format") {
        None | Some(Value::Null) => Format::Text,
        Some(Value::String(s)) if s == "text" => Format::Text,
        Some(Value::String(s)) if s == "json" => Format::Json,
        Some(_) => {
            return Err(ToolError::Failed(
                "format must be \"text\" or \"json\"".to_string(),
            ))
        }
    };
    match name {
        "remember" => {
            require(args, &["content"])?;
            let inp: RememberInput = from_args(args)?;
            let out = engine.remember(inp).map_err(engine_error)?;
            emit(format, &out, || render::render_remember(&out))
        }
        "recall" => {
            require(args, &["query"])?;
            let inp: RecallInput = from_args(args)?;
            let out = engine.recall(&inp).map_err(engine_error)?;
            emit(format, &out, || {
                let clock = engine.clock();
                let text = render::render_recall(&out, &render::clock_fmt(&clock));
                text
            })
        }
        "entity" => {
            require(args, &["name"])?;
            let name = args["name"].as_str().unwrap_or_default();
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(20) as usize;
            let out = engine.entity(name, limit).map_err(engine_error)?;
            emit(format, &out, || {
                let clock = engine.clock();
                let text = render::render_entity(&out, &render::clock_fmt(&clock));
                text
            })
        }
        "invalidate" => {
            require(args, &["id"])?;
            let id = args["id"].as_str().unwrap_or_default();
            let at = args.get("at").and_then(Value::as_str);
            let out = engine.invalidate(id, at).map_err(engine_error)?;
            emit(format, &out, || render::render_change(&out))
        }
        "forget" => {
            require(args, &["id"])?;
            let id = args["id"].as_str().unwrap_or_default();
            let hard = args.get("hard").and_then(Value::as_bool).unwrap_or(false);
            let out = engine.forget(id, hard).map_err(engine_error)?;
            emit(format, &out, || render::render_change(&out))
        }
        "merge_entities" => {
            let keep = args.get("keep").and_then(Value::as_str);
            let merge = args.get("merge").and_then(Value::as_str);
            let entity = args.get("entity").and_then(Value::as_str);
            let alias = args.get("alias").and_then(Value::as_str);
            let out = match (keep, merge, entity, alias) {
                (Some(keep), Some(merge), _, _) => engine.merge_entities(keep, merge),
                (_, _, Some(entity), Some(alias)) => engine.add_alias(entity, alias),
                _ => {
                    return Err(ToolError::Failed(
                        "missing required arguments: keep and merge (or entity and alias)"
                            .to_string(),
                    ))
                }
            }
            .map_err(engine_error)?;
            emit(format, &out, || render::render_change(&out))
        }
        "overview" => {
            let query = args
                .get("query")
                .and_then(Value::as_str)
                .filter(|q| !q.trim().is_empty());
            let k = args.get("k").and_then(Value::as_u64).unwrap_or(5) as usize;
            let out = engine.overview(query, k).map_err(engine_error)?;
            emit(format, &out, || {
                let clock = engine.clock();
                let text = render::render_overview(&out, &render::clock_fmt(&clock));
                text
            })
        }
        "stats" => {
            let out = engine.stats().map_err(engine_error)?;
            emit(format, &out, || render::render_stats(&out))
        }
        _ => Err(ToolError::Unknown),
    }
}

fn emit<T: Serialize>(
    format: Format,
    out: &T,
    text: impl FnOnce() -> String,
) -> std::result::Result<String, ToolError> {
    match format {
        Format::Json => serde_json::to_string_pretty(out)
            .map_err(|e| ToolError::Failed(format!("cannot serialize output: {e}"))),
        Format::Text => Ok(text()),
    }
}

fn from_args<T: DeserializeOwned>(args: &Value) -> std::result::Result<T, ToolError> {
    serde_json::from_value(args.clone())
        .map_err(|e| ToolError::Failed(format!("invalid arguments: {e}")))
}

fn engine_error(e: Error) -> ToolError {
    ToolError::Failed(e.to_string())
}

/// Missing or empty required string arguments, named so the agent can fix it.
fn require(args: &Value, fields: &[&str]) -> std::result::Result<(), ToolError> {
    for field in fields {
        let missing = match args.get(*field) {
            None | Some(Value::Null) => true,
            Some(Value::String(s)) => s.trim().is_empty(),
            Some(_) => false,
        };
        if missing {
            return Err(ToolError::Failed(format!(
                "missing required argument: {field}"
            )));
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ schemas

fn entity_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": {"type": "string", "description": "Canonical name, e.g. \"Mom\", \"Pune\", \"neocortex\"."},
            "type": {"type": "string", "description": "person, place, project, org, tool, concept, ..."},
            "description": {"type": "string", "description": "One clause that disambiguates it, e.g. \"the user's mother\"."},
            "aliases": {"type": "array", "items": {"type": "string"}, "description": "Other names for the same thing, e.g. [\"mother\"]."},
            "same_as": {"type": "string", "description": "Force a merge into this existing entity (name, alias or e<id>)."}
        },
        "required": ["name"]
    })
}

fn relation_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "source": {"type": "string", "description": "Subject entity name (created if unknown)."},
            "predicate": {"type": "string", "description": "Lowercase verb phrase, e.g. \"lives in\", \"works at\", \"depends on\"."},
            "target": {"type": "string", "description": "Object entity name (created if unknown)."},
            "fact": {"type": "string", "description": "Natural sentence, e.g. \"Mom lives in Pune\". Defaults to \"source predicate target\"."},
            "exclusive": {"type": "boolean", "description": "True for single-valued facts (lives in, works at): the previous value is ended and kept as history."},
            "valid_from": {"type": "string", "description": "When the fact became true (ISO date, unix seconds or relative phrase)."},
            "valid_to": {"type": "string", "description": "When it stopped being true; omit while it still holds."}
        },
        "required": ["source", "predicate", "target"]
    })
}

fn format_property() -> Value {
    json!({
        "type": "string",
        "enum": ["text", "json"],
        "description": "text (default) is compact and token-cheap; json returns the full output struct."
    })
}

fn tool_defs() -> Vec<Value> {
    vec![
        json!({
            "name": "remember",
            "description": "Store one durable memory. Use it for atomic facts, events, decisions and their reasons, user preferences, mistakes and their fixes, procedures that worked, and outcomes. Write one self-contained sentence with absolute dates. Pass entities and relations when you know them, and mark single-valued relations (lives in, works at, is married to) exclusive so the old value is kept as history. To correct or replace a memory use supersedes or invalidate instead of writing a contradiction. Never store secrets, credentials or transient chatter.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "content": {"type": "string", "description": "One self-contained sentence with absolute dates: what future-you needs to recall."},
                    "kind": {"type": "string", "enum": engine::KINDS, "description": "Defaults to fact. decision/preference/mistake/procedure are usually worth storing."},
                    "tags": {"type": "array", "items": {"type": "string"}, "description": "Lowercase labels used as filters, e.g. [\"family\", \"relocation\"]."},
                    "importance": {"type": "number", "minimum": 0, "maximum": 1, "description": "0..1; 0.7+ for things the user will ask about again."},
                    "scope": {"type": "string", "description": "Namespace such as \"personal\", \"work\", a repo or a project name."},
                    "source": {"type": "string", "description": "Who or what produced this memory (agent name, user, document)."},
                    "session": {"type": "string", "description": "Conversation or run id, for tracing."},
                    "valid_from": {"type": "string", "description": "When it happened or became true: ISO date/datetime, unix seconds, or a relative phrase like \"yesterday\". Defaults to now."},
                    "valid_to": {"type": "string", "description": "When it stopped being true; omit while it still holds."},
                    "entities": {"type": "array", "items": entity_schema(), "description": "Entities to link. Name them the same way every time so they resolve to one node."},
                    "relations": {"type": "array", "items": relation_schema(), "description": "Structured facts to add to the knowledge graph."},
                    "keywords": {"type": "array", "items": {"type": "string"}, "description": "High-level keys for retrieval, e.g. [\"relocation\", \"family\"]."},
                    "supersedes": {"type": "array", "items": {"type": "string"}, "description": "Memory ids (m<id>) this memory replaces; they are ended and kept as history."},
                    "format": format_property()
                },
                "required": ["content"]
            }
        }),
        json!({
            "name": "recall",
            "description": "Retrieve what is known about a topic: memories, current and ended facts, and entity cards, packed into a token budget. Call it at the start of a task, before decisions that may depend on past context, when the user refers to something from before, and when you are stuck. Filter with kinds/tags/scope/time when you know the shape of the answer; leave filters off to search broadly. Use as_of for \"what was true then\", and include_history for how something changed.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Natural question or keywords, e.g. \"where does Mom live\" or \"deploy process\"."},
                    "k": {"type": "integer", "minimum": 1, "description": "Max memories to return (default 8)."},
                    "budget": {"type": "integer", "minimum": 100, "description": "Approximate token budget for the answer (default 1200)."},
                    "kinds": {"type": "array", "items": {"type": "string", "enum": engine::KINDS}, "description": "Only these memory kinds."},
                    "tags": {"type": "array", "items": {"type": "string"}, "description": "Only memories carrying at least one of these tags."},
                    "scope": {"type": "string", "description": "Only this scope."},
                    "source": {"type": "string", "description": "Only memories written by this source."},
                    "session": {"type": "string", "description": "Only this session."},
                    "since": {"type": "string", "description": "Hard lower bound on valid_from (ISO date or relative phrase)."},
                    "until": {"type": "string", "description": "Hard upper bound on valid_from."},
                    "as_of": {"type": "string", "description": "World-time snapshot: what was true at this instant."},
                    "include_history": {"type": "boolean", "description": "Also rank ended/superseded facts (default false)."},
                    "entities": {"type": "array", "items": {"type": "string"}, "description": "Entity names you already know are relevant."},
                    "keywords": {"type": "array", "items": {"type": "string"}, "description": "High-level keys you already know are relevant."},
                    "format": format_property()
                },
                "required": ["query"]
            }
        }),
        json!({
            "name": "entity",
            "description": "Profile of one entity: type, aliases, description, current facts, past facts, similar entities and a newest-first memory timeline. Use it to check what is known about a person, place or project, and to see how a fact changed over time.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "Entity name, alias or e<id>."},
                    "limit": {"type": "integer", "minimum": 1, "description": "Max timeline memories (default 20)."},
                    "format": format_property()
                },
                "required": ["name"]
            }
        }),
        json!({
            "name": "invalidate",
            "description": "Mark a memory (m<id>) or relation (r<id>) as no longer true from a time, without deleting it. The row stays as history and is not presented as current. Prefer this over writing a contradicting memory.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string", "description": "m<id> for a memory or r<id> for a relation."},
                    "at": {"type": "string", "description": "When it stopped being true (ISO date or relative phrase); default now."},
                    "format": format_property()
                },
                "required": ["id"]
            }
        }),
        json!({
            "name": "forget",
            "description": "Retire a memory: soft by default (expired_at set, kept for history) or hard with hard=true (row, vectors, mentions and evidence deleted). Use for wrong, private or no-longer-wanted memories, not for facts that merely changed.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string", "description": "m<id> of the memory."},
                    "hard": {"type": "boolean", "description": "Delete instead of retiring (default false)."},
                    "format": format_property()
                },
                "required": ["id"]
            }
        }),
        json!({
            "name": "merge_entities",
            "description": "Fix entity resolution. Either merge b into a (keep + merge: aliases, mentions and relations move, b disappears) or add another name to an entity (entity + alias). Use it when the same thing was written under two names and recall splits the facts.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "keep": {"type": "string", "description": "Entity that survives (name, alias or e<id>)."},
                    "merge": {"type": "string", "description": "Entity that is absorbed and removed."},
                    "entity": {"type": "string", "description": "Entity to add an alias to."},
                    "alias": {"type": "string", "description": "New alias name."},
                    "format": format_property()
                },
                "anyOf": [
                    {"required": ["keep", "merge"]},
                    {"required": ["entity", "alias"]}
                ]
            }
        }),
        json!({
            "name": "overview",
            "description": "Global view of the memory graph: the most relevant communities (clusters of related entities) with their central entities and strongest facts. Use it to orient yourself in an unfamiliar knowledge base, or with a query to see the neighbourhood of a topic.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Rank communities by relevance to this; omit for the largest."},
                    "k": {"type": "integer", "minimum": 1, "description": "Max communities (default 5)."},
                    "format": format_property()
                }
            }
        }),
        json!({
            "name": "stats",
            "description": "Counts, database size and embedder status. Cheap; use it to confirm neocortex is reachable and to see how much memory exists.",
            "inputSchema": {
                "type": "object",
                "properties": {"format": format_property()}
            }
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_negotiation_echoes_known_versions() {
        for version in PROTOCOLS {
            let v = initialize(&json!({"protocolVersion": version}));
            assert_eq!(v["protocolVersion"], version);
        }
        let v = initialize(&json!({"protocolVersion": "1999-01-01"}));
        assert_eq!(v["protocolVersion"], PROTOCOLS[0]);
        assert_eq!(v["serverInfo"]["name"], "neocortex");
        assert_eq!(v["serverInfo"]["version"], env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn every_tool_has_an_object_schema() {
        let tools = tool_defs();
        assert_eq!(tools.len(), 8);
        for tool in &tools {
            assert_eq!(tool["inputSchema"]["type"], "object", "{tool}");
            assert!(tool["description"].as_str().unwrap().len() > 20, "{tool}");
        }
    }

    #[test]
    fn missing_arguments_name_the_field() {
        let engine = Engine::open_in_memory(std::sync::Arc::new(EmbedderStub)).unwrap();
        let reply = call_tool(
            &engine,
            &json!({"name": "remember", "arguments": {"content": "  "}}),
        )
        .unwrap();
        assert_eq!(reply["isError"], true);
        assert!(reply["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("content"));
    }

    /// Never embeds; the missing-argument path must not reach the engine.
    struct EmbedderStub;

    impl Embedder for EmbedderStub {
        fn embed(&self, _texts: &[&str], _mode: Mode) -> Result<Vec<Vec<f32>>> {
            Err(Error("stub".into()))
        }

        fn name(&self) -> &str {
            "stub"
        }
    }
}
