//! ReMem v3 graph layer: memory nodes, agent/session hubs, Cypher access.
//!
//! graphqlite is a loadable SQLite extension, so it is loaded into a
//! `rusqlite::Connection` that we create ourselves and hand to graphqlite
//! (`Connection::from_rusqlite`). That keeps one connection on one file
//! holding the graph tables side by side with the relational, FTS5 and vec0
//! tables owned by `remem-store`.

use std::fmt;
use std::path::Path;

use graphqlite::{Connection as GqlConnection, Graph as GqlGraph};
use remem_types::{MemoryItem, MemoryKind};
use serde_json::{json, Value as JsonValue};

/// Label of memory nodes. Properties: `mid` (memory id), `kind`.
pub const MEMORY_LABEL: &str = "Memory";
/// Label of agent hub nodes. Node id is `agent:<aid>`, property `aid`.
pub const AGENT_LABEL: &str = "Agent";
/// Label of session hub nodes. Node id is `session:<sid>`, property `sid`.
pub const SESSION_LABEL: &str = "Session";
/// Relationship type used by [`Graph::link`] when the caller passes none.
pub const DEFAULT_REL: &str = "RELATES_TO";
/// Memory -> Agent hub relationship type.
pub const BELONGS_TO_AGENT: &str = "BELONGS_TO_AGENT";
/// Memory -> Session hub relationship type.
pub const BELONGS_TO_SESSION: &str = "BELONGS_TO_SESSION";
/// Namespace prefix for agent hub node ids.
pub const AGENT_PREFIX: &str = "agent:";
/// Namespace prefix for session hub node ids.
pub const SESSION_PREFIX: &str = "session:";

fn hub_id(prefix: &str, id: &str) -> String {
    format!("{prefix}{id}")
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    /// graphqlite (extension load, Cypher, result decoding) failure.
    Graph(graphqlite::Error),
    /// Plain SQLite failure.
    Sqlite(rusqlite::Error),
    /// A node referenced by an operation does not exist.
    MissingNode(String),
    /// `params_json` was not a JSON object.
    InvalidParams(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Graph(e) => write!(f, "graph error: {e}"),
            Error::Sqlite(e) => write!(f, "sqlite error: {e}"),
            Error::MissingNode(id) => write!(f, "node not found: {id}"),
            Error::InvalidParams(e) => write!(f, "invalid params: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<graphqlite::Error> for Error {
    fn from(e: graphqlite::Error) -> Self {
        Error::Graph(e)
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Sqlite(e)
    }
}

/// One adjacency row from [`Graph::neighbors_detail`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Neighbor {
    /// Business key of the neighbouring node: `mid`, or `aid`/`sid` for hubs.
    pub id: String,
    pub rel: String,
    /// True when the edge points at `id` (outgoing from the queried node).
    pub outgoing: bool,
    /// Node labels of the neighbour, e.g. `["Memory"]` or `["Agent"]`.
    pub labels: Vec<String>,
}

pub struct Graph {
    inner: GqlGraph,
}

impl Graph {
    /// Open (creating if needed) the graph inside the SQLite file at `path`.
    /// Pass `":memory:"` for a throwaway graph. Point this at the store's file:
    /// the graph tables land next to the relational/FTS5/vec0 tables on the
    /// same connection-compatible file (see the coexistence tests).
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let raw = rusqlite::Connection::open(path)?;
        // The store keeps its own connection on the same file; wait out its
        // write transactions instead of failing with SQLITE_BUSY.
        raw.busy_timeout(std::time::Duration::from_secs(5))?;
        // WAL, like the store: two writers on one file need it to overlap.
        // If both connections race to switch the file, the pragma can fail with
        // BUSY even though WAL won, so only propagate when the mode differs.
        if let Err(e) = raw.pragma_update(None, "journal_mode", "WAL") {
            let mode: String = raw
                .pragma_query_value(None, "journal_mode", |r| r.get(0))
                .unwrap_or_default();
            if !mode.eq_ignore_ascii_case("wal") {
                return Err(Error::Sqlite(e));
            }
        }
        Self::from_connection(raw)
    }

    /// In-memory graph.
    pub fn open_in_memory() -> Result<Self> {
        Self::open(":memory:")
    }

    /// Adopt an existing rusqlite connection (loads the graphqlite extension).
    pub fn from_connection(raw: rusqlite::Connection) -> Result<Self> {
        let conn = GqlConnection::from_rusqlite(raw)?;
        Ok(Graph {
            inner: GqlGraph::from_connection(conn),
        })
    }

    /// The underlying rusqlite connection, for coexistence checks and raw SQL.
    pub fn sqlite(&self) -> &rusqlite::Connection {
        self.inner.connection().sqlite_connection()
    }

    /// Insert or update a `Memory` node keyed by memory id.
    ///
    /// ponytail: one Cypher round-trip per property (~3k ops/s). If a full
    /// rebuild from the store ever gets slow, use
    /// `graphqlite::Graph::insert_nodes_bulk` / `insert_edges_bulk`.
    pub fn upsert_memory(&self, id: &str, kind: MemoryKind) -> Result<()> {
        self.inner.upsert_node(
            id,
            [
                ("mid", graphqlite::PropertyValue::Text(id.to_string())),
                (
                    "kind",
                    graphqlite::PropertyValue::Text(kind.as_str().to_string()),
                ),
            ],
            MEMORY_LABEL,
        )?;
        Ok(())
    }

    /// Ensure an `Agent` hub node exists. Hub ids are namespaced (`agent:<aid>`)
    /// because graphqlite keeps one id space for all labels.
    pub fn upsert_agent(&self, aid: &str) -> Result<()> {
        self.inner.upsert_node(
            &hub_id(AGENT_PREFIX, aid),
            [("aid", graphqlite::PropertyValue::Text(aid.to_string()))],
            AGENT_LABEL,
        )?;
        Ok(())
    }

    /// Ensure a `Session` hub node exists (`session:<sid>`).
    pub fn upsert_session(&self, sid: &str) -> Result<()> {
        self.inner.upsert_node(
            &hub_id(SESSION_PREFIX, sid),
            [("sid", graphqlite::PropertyValue::Text(sid.to_string()))],
            SESSION_LABEL,
        )?;
        Ok(())
    }

    /// Project a stored memory onto its node plus agent/session hubs and edges.
    pub fn attach(&self, item: &MemoryItem) -> Result<()> {
        self.upsert_memory(&item.id, item.kind)?;
        if !item.agent_id.is_empty() {
            self.upsert_agent(&item.agent_id)?;
            self.link_unchecked(
                &item.id,
                &hub_id(AGENT_PREFIX, &item.agent_id),
                BELONGS_TO_AGENT,
            )?;
        }
        if !item.session_id.is_empty() {
            self.upsert_session(&item.session_id)?;
            self.link_unchecked(
                &item.id,
                &hub_id(SESSION_PREFIX, &item.session_id),
                BELONGS_TO_SESSION,
            )?;
        }
        Ok(())
    }

    /// Delete a memory node and its edges.
    pub fn forget(&self, id: &str) -> Result<()> {
        self.inner.delete_node(id)?;
        Ok(())
    }

    /// Directed edge `from -> to` of type `rel`. Both nodes must exist;
    /// re-linking the same pair updates the existing edge (MERGE semantics).
    /// Non-identifier characters in `rel` are replaced with `_` by graphqlite,
    /// so read back the type from [`neighbors`](Self::neighbors) rather than
    /// assuming the input spelling.
    pub fn link(&self, from: &str, to: &str, rel: &str) -> Result<()> {
        for id in [from, to] {
            if !self.inner.has_node(id)? {
                return Err(Error::MissingNode(id.to_string()));
            }
        }
        self.link_unchecked(from, to, rel)
    }

    /// Edge with [`DEFAULT_REL`].
    pub fn relates_to(&self, from: &str, to: &str) -> Result<()> {
        self.link(from, to, DEFAULT_REL)
    }

    /// MERGE without the existence pre-check (callers own node creation).
    fn link_unchecked(&self, from: &str, to: &str, rel: &str) -> Result<()> {
        let empty: [(&str, graphqlite::PropertyValue); 0] = [];
        self.inner.upsert_edge(from, to, empty, rel)?;
        Ok(())
    }

    /// Memory-to-memory neighbours of `mid` as `(mid, rel)` pairs, both edge
    /// directions. Agent/Session hub edges are excluded: a hub id is not a
    /// memory id and would poison a recall expansion. Use
    /// [`neighbors_detail`](Self::neighbors_detail) or `cypher` for hubs.
    pub fn neighbors(&self, mid: &str) -> Result<Vec<(String, String)>> {
        Ok(self
            .neighbors_detail(mid)?
            .into_iter()
            .filter(|n| n.labels.iter().any(|l| l == MEMORY_LABEL))
            .map(|n| (n.id, n.rel))
            .collect())
    }

    /// Everything adjacent to `mid`, including hub nodes, with direction and
    /// labels.
    pub fn neighbors_detail(&self, mid: &str) -> Result<Vec<Neighbor>> {
        // graphqlite 0.8 drops all rows for
        // `MATCH (n {id: $id})-[r]-(m:Memory)`, so the neighbour's label is read
        // back with labels(m) instead of matched in the pattern.
        let rows = self
            .inner
            .connection()
            .cypher_builder(
                "MATCH (n {id: $id})-[r]-(m) \
                 RETURN id(n) AS nid, startNode(r) AS src, type(r) AS rel, \
                        m.mid AS mid, m.aid AS aid, m.sid AS sid, \
                        labels(m) AS labels",
            )
            .param("id", mid)
            .run()?;
        let mut out = Vec::new();
        for row in &rows {
            let nid: i64 = row.get("nid").unwrap_or(-1);
            let src: i64 = row.get("src").unwrap_or(-2);
            let labels = match row.get_value("labels") {
                Some(graphqlite::Value::Array(arr)) => arr
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect(),
                _ => Vec::new(),
            };
            let id = ["mid", "aid", "sid"]
                .iter()
                .find_map(|k| row.get_value(k).and_then(|v| v.as_str()))
                .unwrap_or_default()
                .to_string();
            out.push(Neighbor {
                id,
                rel: row.get("rel").unwrap_or_default(),
                outgoing: src == nid,
                labels,
            });
        }
        Ok(out)
    }

    /// Run a Cypher query. `params` is a JSON object of `$name` bindings, or
    /// `Value::Null` for none. Returns rows as a JSON array of objects.
    pub fn cypher(&self, query: &str, params: &JsonValue) -> Result<JsonValue> {
        let rows = match params {
            JsonValue::Null => self.inner.query(query)?,
            JsonValue::Object(map) => self
                .inner
                .query_builder(query)
                .params(&JsonValue::Object(map.clone()))
                .run()?,
            other => {
                return Err(Error::InvalidParams(format!(
                    "expected a JSON object or null, got {other}"
                )))
            }
        };
        let mut out = Vec::with_capacity(rows.len());
        for row in &rows {
            let mut obj = serde_json::Map::new();
            for col in row.columns() {
                let value = row
                    .get_value(col)
                    .cloned()
                    .unwrap_or(graphqlite::Value::Null);
                obj.insert(col.clone(), json_of(value));
            }
            out.push(JsonValue::Object(obj));
        }
        Ok(JsonValue::Array(out))
    }

    /// Node/edge counts, for `remem stats`.
    pub fn stats(&self) -> Result<JsonValue> {
        let s = self.inner.stats()?;
        Ok(json!({
            "nodes": s.node_count,
            "edges": s.edge_count,
        }))
    }
}

/// Decode a graphqlite value into JSON. graphqlite's `Value` is an untagged
/// serde enum but its `Object` variant holds a HashMap, so convert by hand to
/// keep key insertion order from the row.
fn json_of(value: graphqlite::Value) -> JsonValue {
    use graphqlite::Value as V;
    match value {
        V::Null => JsonValue::Null,
        V::Bool(b) => JsonValue::Bool(b),
        V::Integer(i) => JsonValue::from(i),
        V::Float(f) => JsonValue::from(f),
        V::String(s) => JsonValue::String(s),
        V::Array(arr) => JsonValue::Array(arr.into_iter().map(json_of).collect()),
        V::Object(map) => {
            let mut obj = serde_json::Map::new();
            for (k, v) in map {
                obj.insert(k, json_of(v));
            }
            JsonValue::Object(obj)
        }
    }
}
