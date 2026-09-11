//! ReMem v3 graph layer: memory nodes, agent/session hubs, Cypher access.
//!
//! graphqlite is a loadable SQLite extension, so it is loaded into a
//! `rusqlite::Connection` that we create ourselves and hand to graphqlite
//! (`Connection::from_rusqlite`). That keeps one connection on one file
//! holding the graph tables side by side with the relational, FTS5 and vec0
//! tables owned by `remem-store`.

use std::collections::HashMap;
use std::fmt;
use std::path::Path;

use graphqlite::{Connection as GqlConnection, Graph as GqlGraph};
use remem_types::{MemoryItem, MemoryKind};
use serde_json::{json, Value as JsonValue};

/// Edge property key carrying [`EdgeProvenance`]. Memory-to-memory edges get
/// it at write time; hub edges and pre-provenance rows have no value and read
/// back as [`EdgeProvenance::Manual`].
pub const PROVENANCE_KEY: &str = "provenance";

/// Where a graph edge came from (issue #15). Stored as a text edge property;
/// accepted values are exactly the three below — anything else is rejected at
/// the write API so a typo cannot silently become a fourth provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeProvenance {
    /// Hand-made via `link` (the default).
    Manual,
    /// A future recall-time proposer (accepted and stored, no producer yet).
    RecallSuggested,
    /// Written by a correction-chain / supersede operation.
    CorrectionChain,
}

impl EdgeProvenance {
    pub fn as_str(self) -> &'static str {
        match self {
            EdgeProvenance::Manual => "manual",
            EdgeProvenance::RecallSuggested => "recall-suggested",
            EdgeProvenance::CorrectionChain => "correction-chain",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "manual" => Some(EdgeProvenance::Manual),
            "recall-suggested" => Some(EdgeProvenance::RecallSuggested),
            "correction-chain" => Some(EdgeProvenance::CorrectionChain),
            _ => None,
        }
    }
}

impl fmt::Display for EdgeProvenance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

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

/// Reject memory ids that would collide with hub node ids. graphqlite keeps one
/// id space for all labels, and hubs live at `agent:<aid>` / `session:<sid>`.
/// Only these two prefixes are reserved (not `:` generally): UUIDs and ids like
/// `foo:bar` never collide, so rejecting more would break legit ids for nothing.
fn reject_reserved_memory_id(id: &str) -> Result<()> {
    if id.starts_with(AGENT_PREFIX) || id.starts_with(SESSION_PREFIX) {
        return Err(Error::ReservedPrefix(id.to_string()));
    }
    Ok(())
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
    /// A memory id using a reserved hub prefix (`agent:`/`session:`).
    ReservedPrefix(String),
    /// `link_with_provenance_str` got a value outside the three known ones.
    UnknownProvenance(String),
    /// `params_json` was not a JSON object.
    InvalidParams(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Graph(e) => write!(f, "graph error: {e}"),
            Error::Sqlite(e) => write!(f, "sqlite error: {e}"),
            Error::MissingNode(id) => write!(f, "node not found: {id}"),
            Error::ReservedPrefix(id) => write!(
                f,
                "reserved id prefix: {id} (memory ids may not start with `agent:` or `session:`)"
            ),
            Error::InvalidParams(e) => write!(f, "invalid params: {e}"),
            Error::UnknownProvenance(s) => write!(
                f,
                "unknown provenance: {s} (manual|recall-suggested|correction-chain)"
            ),
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
    /// Edge provenance; rows written before provenance existed read as manual.
    pub provenance: EdgeProvenance,
    /// True when the edge points at `id` (outgoing from the queried node).
    pub outgoing: bool,
    /// Node labels of the neighbour, e.g. `["Memory"]` or `["Agent"]`.
    pub labels: Vec<String>,
}

/// One node of the [`Graph::edges`] snapshot: a memory (id = mid, kind = memory
/// kind) or a hub (id = `agent:<aid>`/`session:<sid>`, kind = label).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VizNode {
    pub id: String,
    pub kind: String,
}

/// One directed edge of the [`Graph::edges`] snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VizEdge {
    pub from: String,
    pub rel: String,
    pub to: String,
}

/// One memory-to-memory edge in stored orientation (both ends are Memory
/// nodes; hub edges excluded).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryEdge {
    pub from: String,
    pub to: String,
    pub rel: String,
    pub provenance: EdgeProvenance,
}

/// One discovered node of [`Graph::impact`]: the memory reached, the edge
/// that found it, and the BFS depth from the seed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactHit {
    /// Memory id of the reached node (never the seed).
    pub id: String,
    /// Relation of the discovering edge.
    pub rel: String,
    /// Provenance of the discovering edge.
    pub provenance: EdgeProvenance,
    /// BFS depth from the seed (first hop = 1).
    pub depth: usize,
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
    /// Rejects ids starting with `agent:`/`session:`: they would share a node id
    /// with an Agent/Session hub in graphqlite's single id space. `attach` inherits
    /// the rejection through this call.
    ///
    /// ponytail: one Cypher round-trip per property (~3k ops/s). If a full
    /// rebuild from the store ever gets slow, use
    /// `graphqlite::Graph::insert_nodes_bulk` / `insert_edges_bulk`.
    pub fn upsert_memory(&self, id: &str, kind: MemoryKind) -> Result<()> {
        reject_reserved_memory_id(id)?;
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
                None,
            )?;
        }
        if !item.session_id.is_empty() {
            self.upsert_session(&item.session_id)?;
            self.link_unchecked(
                &item.id,
                &hub_id(SESSION_PREFIX, &item.session_id),
                BELONGS_TO_SESSION,
                None,
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
    /// assuming the input spelling. Records [`EdgeProvenance::Manual`].
    pub fn link(&self, from: &str, to: &str, rel: &str) -> Result<()> {
        self.link_with_provenance(from, to, rel, EdgeProvenance::Manual)
    }

    /// Directed edge with an explicit provenance. Unknown provenance strings
    /// are rejected (see [`link_with_provenance_str`](Self::link_with_provenance_str)).
    pub fn link_with_provenance(
        &self,
        from: &str,
        to: &str,
        rel: &str,
        provenance: EdgeProvenance,
    ) -> Result<()> {
        for id in [from, to] {
            if !self.inner.has_node(id)? {
                return Err(Error::MissingNode(id.to_string()));
            }
        }
        self.link_unchecked(from, to, rel, Some(provenance))
    }

    /// [`link_with_provenance`](Self::link_with_provenance) from a raw string
    /// (CLI/MCP input): `"manual" | "recall-suggested" | "correction-chain"`.
    pub fn link_with_provenance_str(
        &self,
        from: &str,
        to: &str,
        rel: &str,
        provenance: &str,
    ) -> Result<()> {
        match EdgeProvenance::parse(provenance) {
            Some(p) => self.link_with_provenance(from, to, rel, p),
            None => Err(Error::UnknownProvenance(provenance.to_string())),
        }
    }

    /// Edge with [`DEFAULT_REL`].
    pub fn relates_to(&self, from: &str, to: &str) -> Result<()> {
        self.link(from, to, DEFAULT_REL)
    }

    /// MERGE without the existence pre-check (callers own node creation).
    /// `None` writes no provenance (hub edges); memory edges pass `Some`.
    fn link_unchecked(
        &self,
        from: &str,
        to: &str,
        rel: &str,
        provenance: Option<EdgeProvenance>,
    ) -> Result<()> {
        match provenance {
            Some(p) => self.inner.upsert_edge(
                from,
                to,
                [(
                    PROVENANCE_KEY,
                    graphqlite::PropertyValue::Text(p.to_string()),
                )],
                rel,
            )?,
            None => {
                let empty: [(&str, graphqlite::PropertyValue); 0] = [];
                self.inner.upsert_edge(from, to, empty, rel)?;
            }
        }
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
                        r.provenance AS prov, \
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
            let provenance = match row.get_value("prov") {
                Some(graphqlite::Value::String(s)) => {
                    EdgeProvenance::parse(s).unwrap_or(EdgeProvenance::Manual)
                }
                _ => EdgeProvenance::Manual,
            };
            out.push(Neighbor {
                id,
                rel: row.get("rel").unwrap_or_default(),
                provenance,
                outgoing: src == nid,
                labels,
            });
        }
        Ok(out)
    }

    /// Structural health check: human-readable lines describing edges that point
    /// at nodes which no longer exist, and `Memory` nodes with no memory-to-memory
    /// edge (agent/session hub edges alone do not count). Empty means healthy.
    ///
    /// graphqlite declares `ON DELETE CASCADE` and opens its connection with
    /// `PRAGMA foreign_keys = ON`, so this crate's own API cannot create a dangling
    /// edge. The check exists for what that layer cannot cover: another writer on
    /// the same file (the pragma is per connection) or schema drift. Ordering is
    /// deterministic: dangling edges by rowid, then orphans by memory id.
    ///
    /// SQL failures become lines rather than `Err`: callers print the result, and a
    /// broken query must not read back as a healthy graph.
    pub fn validate(&self) -> Vec<String> {
        let mut out = Vec::new();
        match self.dangling_edges() {
            Ok(rows) => out.extend(rows),
            Err(e) => return vec![format!("validation query failed: {e}")],
        }
        match self.unreviewed_edges() {
            Ok(rows) => out.extend(rows),
            Err(e) => out.push(format!("validation query failed: {e}")),
        }
        match self.orphan_memories() {
            Ok(rows) => out.extend(rows),
            Err(e) => out.push(format!("validation query failed: {e}")),
        }
        out
    }

    /// Every memory-to-memory edge in stored orientation with its provenance.
    /// Rows written before provenance existed read as manual. Ordered by edge
    /// rowid (deterministic).
    pub fn memory_edges(&self) -> Result<Vec<MemoryEdge>> {
        let mut stmt = self.sqlite().prepare(
            "SELECT e.source_id, e.target_id, e.type, p.value              FROM edges e              LEFT JOIN edge_props_text p ON p.edge_id = e.id                AND p.key_id = (SELECT id FROM property_keys WHERE key = 'provenance')              ORDER BY e.id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })?;
        let names = self.node_names()?;
        let mut out = Vec::new();
        for row in rows {
            let (src, dst, rel, prov) = row?;
            let (Some(from), Some(to)) = (names.get(&src), names.get(&dst)) else {
                continue; // dangling end: covered by dangling_edges()
            };
            if !hub_id_is_memory(from) || !hub_id_is_memory(to) {
                continue; // hub edges carry no provenance
            }
            out.push(MemoryEdge {
                from: from.clone(),
                to: to.clone(),
                rel,
                provenance: prov
                    .as_deref()
                    .and_then(EdgeProvenance::parse)
                    .unwrap_or(EdgeProvenance::Manual),
            });
        }
        Ok(out)
    }

    /// Lines for memory edges whose provenance is not manual, e.g.
    /// `unreviewed edge: m1 -[:SUPERSEDES]-> m2 (provenance: correction-chain)`.
    /// Rows without a provenance value (pre-feature, hub edges) read as manual
    /// and stay silent, so old databases validate clean.
    fn unreviewed_edges(&self) -> Result<Vec<String>> {
        Ok(self
            .memory_edges()?
            .into_iter()
            .filter(|e| e.provenance != EdgeProvenance::Manual)
            .map(|e| {
                format!(
                    "unreviewed edge: {} -[:{}]-> {} (provenance: {})",
                    e.from, e.rel, e.to, e.provenance
                )
            })
            .collect())
    }

    /// Lines for edges with a missing end, e.g.
    /// `dangling edge: m1 -[:RELATES_TO]-> #3 (missing target node)`. A missing
    /// end is labelled by rowid (its properties are gone); a surviving end keeps
    /// its `mid` or namespaced hub id.
    fn dangling_edges(&self) -> Result<Vec<String>> {
        let mut stmt = self.sqlite().prepare(
            "SELECT e.source_id, e.target_id, e.type, (sn.id IS NULL), (tn.id IS NULL) \
             FROM edges e \
             LEFT JOIN nodes sn ON sn.id = e.source_id \
             LEFT JOIN nodes tn ON tn.id = e.target_id \
             WHERE sn.id IS NULL OR tn.id IS NULL \
             ORDER BY e.id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, bool>(3)?,
                r.get::<_, bool>(4)?,
            ))
        })?;
        let edges: Vec<(i64, i64, String, bool, bool)> =
            rows.collect::<std::result::Result<_, _>>()?;
        if edges.is_empty() {
            return Ok(Vec::new());
        }
        let names = self.node_names()?;
        let label = |id: i64, gone: bool| match gone {
            true => format!("#{id}"),
            false => names.get(&id).cloned().unwrap_or_else(|| format!("#{id}")),
        };
        Ok(edges
            .into_iter()
            .map(|(src, dst, rel, src_gone, dst_gone)| {
                let what = match (src_gone, dst_gone) {
                    (true, true) => "missing source and target nodes",
                    (true, false) => "missing source node",
                    (false, true) => "missing target node",
                    (false, false) => "missing node",
                };
                format!(
                    "dangling edge: {} -[:{rel}]-> {} ({what})",
                    label(src, src_gone),
                    label(dst, dst_gone)
                )
            })
            .collect())
    }

    /// Rowid -> display id for every node: a `mid`, or a namespaced hub id.
    fn node_names(&self) -> Result<std::collections::HashMap<i64, String>> {
        let mut stmt = self.sqlite().prepare(&format!(
            "SELECT n.id, COALESCE( \
                    (SELECT p.value FROM node_props_text p JOIN property_keys k ON k.id = p.key_id \
                     WHERE p.node_id = n.id AND k.key = 'mid'), \
                    (SELECT '{AGENT_PREFIX}' || p.value FROM node_props_text p \
                     JOIN property_keys k ON k.id = p.key_id \
                     WHERE p.node_id = n.id AND k.key = 'aid'), \
                    (SELECT '{SESSION_PREFIX}' || p.value FROM node_props_text p \
                     JOIN property_keys k ON k.id = p.key_id \
                     WHERE p.node_id = n.id AND k.key = 'sid')) \
                 FROM nodes n"
        ))?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?))
        })?;
        let mut map = std::collections::HashMap::new();
        for row in rows {
            if let (id, Some(name)) = row? {
                map.insert(id, name);
            }
        }
        Ok(map)
    }

    /// Lines for `Memory` nodes whose only edges (if any) touch hub nodes.
    fn orphan_memories(&self) -> Result<Vec<String>> {
        let mut stmt = self.sqlite().prepare(&format!(
            "SELECT p.value AS mid FROM node_props_text p \
                 JOIN property_keys k ON k.id = p.key_id \
                 WHERE k.key = 'mid' \
                 AND NOT EXISTS ( \
                    SELECT 1 FROM edges e WHERE e.source_id = p.node_id \
                    AND e.target_id NOT IN (SELECT node_id FROM node_labels \
                        WHERE label IN ('{AGENT_LABEL}', '{SESSION_LABEL}'))) \
                 AND NOT EXISTS ( \
                    SELECT 1 FROM edges e WHERE e.target_id = p.node_id \
                    AND e.source_id NOT IN (SELECT node_id FROM node_labels \
                        WHERE label IN ('{AGENT_LABEL}', '{SESSION_LABEL}'))) \
                 ORDER BY mid"
        ))?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(format!(
                "orphan memory: {} (no edges outside agent/session hubs)",
                row?
            ));
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

    /// Memory nodes ranked by importance, highest score first. Runs graphqlite's
    /// PageRank over the whole graph (agent/session hubs included, so a hub that
    /// anchors many memories also ranks), restricted to `Memory`-labelled nodes
    /// and keyed by `mid`. An empty graph returns an empty vector.
    pub fn central(&self) -> Result<Vec<(String, f64)>> {
        // INVARIANT: `upsert_memory` rejects `agent:`/`session:` ids, so any
        // prefixed node id is a hub, never a memory. The prefix filter below is
        // therefore exact (no silent drop of a real memory).
        Ok(self
            .inner
            .pagerank(0.85, 20)?
            .into_iter()
            .filter(|r| r.user_id.as_deref().is_some_and(hub_id_is_memory))
            .map(|r| (r.user_id.unwrap(), r.score))
            .collect())
    }

    /// Shortest memory-to-memory path between `from` and `to`, returned in
    /// stored orientation as `[first, .., last]`. graphqlite's Dijkstra is
    /// DIRECTIONAL, so a forward miss falls back to the reverse query
    /// (`to -> from`) and the found path is returned as-is: callers use this
    /// as a symmetric "path between" tool. Missing endpoints or a target
    /// unreachable in either direction return an empty vector, not an error.
    pub fn shortest_path(&self, from: &str, to: &str) -> Result<Vec<String>> {
        let sp = self.inner.shortest_path(from, to, None)?;
        if sp.found {
            return Ok(sp.path);
        }
        let sp = self.inner.shortest_path(to, from, None)?;
        Ok(match sp.found {
            true => sp.path,
            false => Vec::new(),
        })
    }

    /// Whole-graph snapshot for visualization: every node (memories + hubs) and
    /// every directed edge. Nodes ordered by id, edges by (from, rel, to), for
    /// stable output. `kind` is the memory kind for `Memory` nodes and the hub
    /// label (`Agent`/`Session`) for hubs.
    pub fn edges(&self) -> Result<(Vec<VizNode>, Vec<VizEdge>)> {
        let node_rows = self.cypher(
            "MATCH (n) RETURN n.id AS id, labels(n) AS labels, n.kind AS kind ORDER BY id",
            &JsonValue::Null,
        )?;
        let edge_rows = self.cypher(
            "MATCH (a)-[r]->(b)              RETURN a.id AS src, type(r) AS rel, b.id AS tgt              ORDER BY src, rel, tgt",
            &JsonValue::Null,
        )?;
        let JsonValue::Array(node_rows) = node_rows else {
            unreachable!("cypher returns a JSON array of rows")
        };
        let mut nodes = Vec::with_capacity(node_rows.len());
        for row in node_rows {
            let id = row
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let labels: Vec<String> = row
                .get("labels")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let kind = row
                .get("kind")
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or_else(|| labels.first().cloned().unwrap_or_default());
            nodes.push(VizNode { id, kind });
        }
        let JsonValue::Array(edge_rows) = edge_rows else {
            unreachable!("cypher returns a JSON array of rows")
        };
        let mut edges = Vec::with_capacity(edge_rows.len());
        for row in edge_rows {
            let str_at = |k: &str| {
                row.get(k)
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string()
            };
            edges.push(VizEdge {
                from: str_at("src"),
                rel: str_at("rel"),
                to: str_at("tgt"),
            });
        }
        Ok((nodes, edges))
    }

    /// Directional BFS over memory-to-memory edges from `seed` (donor:
    /// graphify `affected.py::affected_nodes`): both edge directions, depth-
    /// limited, optional relation filter. Incoming edges are dependents,
    /// outgoing edges are dependencies; SUPERSEDES edges surface superseded
    /// rows next to their live successors. The seed itself is never reported.
    /// Unknown id -> empty node list. Deterministic: breadth-first, discovery
    /// order within a depth follows `memory_edges()` rowid order.
    pub fn impact(&self, seed: &str, depth: usize, rel: Option<&str>) -> Result<Vec<ImpactHit>> {
        let edges = self.memory_edges()?;
        let mut adj: HashMap<&str, Vec<&MemoryEdge>> = HashMap::new();
        for e in &edges {
            if rel.is_some_and(|r| e.rel != r) {
                continue;
            }
            adj.entry(e.from.as_str()).or_default().push(e);
            adj.entry(e.to.as_str()).or_default().push(e);
        }
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        seen.insert(seed);
        let mut frontier: Vec<(&str, &MemoryEdge)> = Vec::new();
        if let Some(first) = adj.get(seed) {
            for e in first {
                let next = if e.from == seed {
                    e.to.as_str()
                } else {
                    e.from.as_str()
                };
                if seen.insert(next) {
                    frontier.push((next, e));
                }
            }
        }
        let mut out = Vec::new();
        let mut current = frontier;
        let mut d = 1usize;
        while d <= depth && !current.is_empty() {
            let mut next_wave: Vec<(&str, &MemoryEdge)> = Vec::new();
            for (id, edge) in &current {
                out.push(ImpactHit {
                    id: (*id).to_string(),
                    rel: edge.rel.clone(),
                    provenance: edge.provenance,
                    depth: d,
                });
            }
            d += 1;
            for (id, _) in &current {
                if let Some(hood) = adj.get(*id) {
                    for e in hood {
                        let next = if e.from == *id {
                            e.to.as_str()
                        } else {
                            e.from.as_str()
                        };
                        if seen.insert(next) {
                            next_wave.push((next, e));
                        }
                    }
                }
            }
            current = next_wave;
        }
        Ok(out)
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

/// True when a graph node id names a Memory node: hubs are namespaced
/// (`agent:`/`session:`), memories are not.
fn hub_id_is_memory(id: &str) -> bool {
    !id.is_empty() && !id.starts_with(AGENT_PREFIX) && !id.starts_with(SESSION_PREFIX)
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
