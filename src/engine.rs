//! The public API: `Engine` plus the input/output types shared by the CLI and
//! the MCP server. Inputs deserialize straight from MCP tool arguments, so
//! field names here are the tool schema.

use crate::db::Db;
use crate::embed::Embedder;
use crate::error::Result;
use crate::temporal::Clock;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

pub const KINDS: &[&str] = &[
    "fact",
    "event",
    "decision",
    "preference",
    "mistake",
    "procedure",
    "observation",
    "note",
];

/// Tunables. Defaults are the design's starting values; eval calibrates them.
#[derive(Clone, Debug)]
pub struct Config {
    /// Cosine at/above which a new memory is reported as a near duplicate.
    pub near_dup: f32,
    /// Entity cosine at/above which a soft `similar` edge is written.
    pub sim_edge: f32,
    /// Minimum entity cosine for a query-time graph seed.
    pub entity_seed: f32,
    /// Predicate cosine at/above which a new predicate becomes an alias.
    pub pred_alias: f32,
    pub w_bm25: f64,
    pub w_vector: f64,
    pub w_graph: f64,
    pub w_temporal: f64,
    pub rrf_k: f64,
    /// PPR teleport probability.
    pub alpha: f64,
    /// PPR push residual threshold.
    pub eps: f64,
    /// Score multiplier for expired/ended memories without history intent.
    pub expired_penalty: f64,
    pub mmr_lambda: f64,
    /// Recency half-life in days (applies to valid_from).
    pub half_life_days: f64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            near_dup: 0.97,
            sim_edge: 0.75,
            entity_seed: 0.55,
            pred_alias: 0.90,
            w_bm25: 1.0,
            w_vector: 1.0,
            w_graph: 1.0,
            w_temporal: 1.0,
            rrf_k: 60.0,
            alpha: 0.5,
            eps: 1e-5,
            expired_penalty: 0.35,
            mmr_lambda: 0.75,
            half_life_days: 90.0,
        }
    }
}

pub struct Engine {
    pub db: Db,
    pub emb: Arc<dyn Embedder>,
    pub cfg: Config,
    /// Fixed "now" for tests/eval; None = system clock.
    pub fixed_clock: Option<Clock>,
}

impl Engine {
    pub fn open(path: &Path, emb: Arc<dyn Embedder>) -> Result<Engine> {
        Ok(Engine {
            db: Db::open(path)?,
            emb,
            cfg: Config::default(),
            fixed_clock: None,
        })
    }

    pub fn open_in_memory(emb: Arc<dyn Embedder>) -> Result<Engine> {
        Ok(Engine {
            db: Db::open_in_memory()?,
            emb,
            cfg: Config::default(),
            fixed_clock: None,
        })
    }

    pub fn with_clock(mut self, clock: Clock) -> Engine {
        self.fixed_clock = Some(clock);
        self
    }

    pub fn clock(&self) -> Clock {
        self.fixed_clock.unwrap_or_else(Clock::system)
    }
}

/// Parse "m12" / "12" (prefix optional) into 12. `prefix` is 'm', 'e' or 'r'.
pub fn parse_id(s: &str, prefix: char) -> Option<i64> {
    let s = s.trim();
    let s = s.strip_prefix(prefix).unwrap_or(s);
    s.parse().ok().filter(|v: &i64| *v > 0)
}

// ---------------------------------------------------------------- remember

#[derive(Clone, Debug, Default, Deserialize)]
pub struct EntityIn {
    pub name: String,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Force-merge into this existing entity (name, alias or e<id>).
    #[serde(default)]
    pub same_as: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct RelationIn {
    #[serde(alias = "src", alias = "subject")]
    pub source: String,
    #[serde(alias = "relation", alias = "rel")]
    pub predicate: String,
    #[serde(alias = "dst", alias = "object")]
    pub target: String,
    /// Natural-language statement; default "source predicate target".
    #[serde(default, alias = "description")]
    pub fact: Option<String>,
    /// (source, predicate) holds at most one target at a time.
    #[serde(default)]
    pub exclusive: bool,
    #[serde(default)]
    pub valid_from: Option<String>,
    #[serde(default)]
    pub valid_to: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct RememberInput {
    #[serde(alias = "text")]
    pub content: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub importance: Option<f32>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub session: Option<String>,
    /// When it happened / became true: ISO date(time), unix seconds or a
    /// relative phrase ("yesterday", "last month").
    #[serde(default, alias = "occurred_at", alias = "when")]
    pub valid_from: Option<String>,
    #[serde(default)]
    pub valid_to: Option<String>,
    #[serde(default)]
    pub entities: Vec<EntityIn>,
    #[serde(default)]
    pub relations: Vec<RelationIn>,
    /// LightRAG-style high-level keywords; indexed with the tags.
    #[serde(default)]
    pub keywords: Vec<String>,
    /// Memory ids (m<id>) this memory replaces.
    #[serde(default)]
    pub supersedes: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct EntityResolution {
    pub name: String,
    pub id: i64,
    /// "new" | "matched" | "alias" | "lexical" | "acronym" | "same_as"
    pub status: String,
    /// Soft links written for a new entity: (entity id, name, cosine).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub similar: Vec<(i64, String, f32)>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RelationResult {
    pub id: i64,
    pub fact: String,
    /// "new" | "merged"
    pub status: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct RememberOutput {
    pub id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duplicate_of: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub near_duplicate: Option<(i64, f32)>,
    pub entities: Vec<EntityResolution>,
    pub relations: Vec<RelationResult>,
    /// Relation ids ended by exclusive writes, and memory ids superseded.
    pub invalidated_relations: Vec<i64>,
    pub superseded_memories: Vec<i64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hints: Vec<String>,
}

// ---------------------------------------------------------------- recall

#[derive(Clone, Debug, Default, Deserialize)]
pub struct RecallInput {
    #[serde(alias = "q", alias = "text")]
    pub query: String,
    #[serde(default)]
    pub k: Option<usize>,
    /// Approximate token budget for the whole answer (chars / 4).
    #[serde(default)]
    pub budget: Option<usize>,
    #[serde(default)]
    pub kinds: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub session: Option<String>,
    /// Hard bounds on valid_from (instant expressions).
    #[serde(default)]
    pub since: Option<String>,
    #[serde(default)]
    pub until: Option<String>,
    /// World-time snapshot: what was true at this instant.
    #[serde(default)]
    pub as_of: Option<String>,
    #[serde(default)]
    pub include_history: bool,
    /// Agent-supplied low-level keys (entity names) and high-level keywords.
    #[serde(default)]
    pub entities: Vec<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MemoryHit {
    pub id: i64,
    pub kind: String,
    pub content: String,
    pub valid_from: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<i64>,
    /// Retired (forgotten/superseded) or ended.
    pub ended: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<i64>,
    pub tags: Vec<String>,
    pub score: f64,
    /// e.g. ["bm25#2", "vec#1", "graph#1", "time"]
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct FactHit {
    pub id: i64,
    pub source: String,
    pub predicate: String,
    pub target: String,
    pub fact: String,
    pub valid_from: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<i64>,
    pub ended: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct EntityCard {
    pub id: i64,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub aliases: Vec<String>,
    pub description: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct RecallOutput {
    pub memories: Vec<MemoryHit>,
    pub facts: Vec<FactHit>,
    pub entities: Vec<EntityCard>,
    /// Matched time window, if the query had one: (start, end, text).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<(i64, i64, String)>,
    /// Candidates considered before packing.
    pub candidates: usize,
    pub tokens_used: usize,
    pub tokens_budget: usize,
    /// Memories dropped by the budget.
    pub omitted: usize,
}

// ---------------------------------------------------------------- others

#[derive(Clone, Debug, Serialize)]
pub struct EntityOutput {
    pub card: EntityCard,
    pub current_facts: Vec<FactHit>,
    pub past_facts: Vec<FactHit>,
    pub similar: Vec<(i64, String, f32)>,
    /// Newest first.
    pub timeline: Vec<MemoryHit>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Stats {
    pub memories: i64,
    pub live_memories: i64,
    pub entities: i64,
    pub relations: i64,
    pub live_relations: i64,
    pub similar_edges: i64,
    pub predicates: i64,
    pub db_bytes: i64,
    pub embedder: String,
    pub version: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ChangeOutput {
    /// What changed, one line per item, e.g. "m12 valid_to=2026-10-01".
    pub changed: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Community {
    pub id: i64,
    pub size: usize,
    /// Most central / most relevant entities first.
    pub entities: Vec<EntityCard>,
    pub facts: Vec<FactHit>,
    pub score: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct OverviewOutput {
    pub communities: Vec<Community>,
}

// ---------------------------------------------------------------- methods

impl Engine {
    /// Store a memory (design §6).
    pub fn remember(&self, _inp: RememberInput) -> Result<RememberOutput> {
        todo!("phase 2: write lane")
    }

    /// Retrieve (design §8).
    pub fn recall(&self, _inp: &RecallInput) -> Result<RecallOutput> {
        todo!("phase 2: read lane")
    }

    /// Profile + timeline of one entity, by name, alias or e<id>.
    pub fn entity(&self, _name_or_id: &str, _limit: usize) -> Result<EntityOutput> {
        todo!("phase 2: read lane")
    }

    /// End validity of a memory (m<id>) or relation (r<id>) at `at` (default now).
    pub fn invalidate(&self, _id: &str, _at: Option<&str>) -> Result<ChangeOutput> {
        todo!("phase 2: write lane")
    }

    /// Retire a memory (soft) or delete it with vectors/mentions/evidence (hard).
    pub fn forget(&self, _id: &str, _hard: bool) -> Result<ChangeOutput> {
        todo!("phase 2: write lane")
    }

    /// Merge entity `merge` into `keep` (aliases, mentions, relations move).
    pub fn merge_entities(&self, _keep: &str, _merge: &str) -> Result<ChangeOutput> {
        todo!("phase 2: write lane")
    }

    /// Add an alias name to an entity.
    pub fn add_alias(&self, _entity: &str, _alias: &str) -> Result<ChangeOutput> {
        todo!("phase 2: write lane")
    }

    /// Communities of the entity graph, ranked by relevance to `query` (or size).
    pub fn overview(&self, _query: Option<&str>, _k: usize) -> Result<OverviewOutput> {
        todo!("phase 2: read lane")
    }

    pub fn stats(&self) -> Result<Stats> {
        todo!("phase 2: write lane")
    }

    /// Rebuild the graphqlite projection from the relational tables.
    pub fn reindex(&self) -> Result<ChangeOutput> {
        todo!("phase 2: write lane")
    }

    /// Integrity problems (projection drift, orphan vectors, dangling refs); empty = healthy.
    pub fn validate(&self) -> Result<Vec<String>> {
        todo!("phase 2: write lane")
    }

    /// Read-only Cypher; rejects CREATE/MERGE/SET/DELETE/REMOVE/DROP.
    pub fn cypher(&self, _query: &str) -> Result<serde_json::Value> {
        todo!("phase 2: write lane")
    }
}
