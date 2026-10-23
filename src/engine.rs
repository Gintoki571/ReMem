//! The public API: `Engine` plus the input/output types shared by the CLI and
//! the MCP server. Inputs deserialize straight from MCP tool arguments, so
//! field names here are the tool schema.

use crate::db::Db;
use crate::embed::Embedder;
use crate::error::Result;
use crate::temporal::Clock;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;

pub type SnapshotCache = Mutex<Option<(i64, Option<i64>, bool, Arc<crate::kg::Snapshot>)>>;

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

/// Tunables. Read-time defaults were calibrated with `neocortex eval --sweep`
/// on eval/fixtures.json and checked on the independent eval/holdout.json
/// (2026-10-23): low RRF k lets a strong single-channel hit win (multi-hop
/// answers found only by PPR), and vector 2 / graph 1.5 keeps PPR from
/// overriding dense similarity on plain paraphrases.
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
    /// RRF weight of semantic multi-hop paths (`paths::search_semantic`).
    pub w_sempath: f64,
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
    /// Keep BM25 hits scoring at least this fraction of the best hit
    /// (0.0 = keep every hit up to the channel depth).
    pub bm25_rel: f64,
    /// Shortest token length that gets a prefix arm in the FTS query.
    pub fts_prefix_min: usize,
    /// Share of PPR seed mass given to the top dense-retrieval memories.
    pub mem_seed_share: f64,
    /// How many top dense-retrieval memories seed PPR.
    pub mem_seed_top: usize,
    /// Boost relation endpoints that are also exact query entity seeds.
    pub fact_entity_boost: f64,
    /// Share of a relation-hit score used to seed its evidence memories.
    pub fact_memory_seed: f64,
    /// Score multiplier for ended memories when the query asks about the past.
    pub history_boost: f64,
    /// Cosine threshold for text-only update candidates.
    pub update_candidate: f32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            near_dup: 0.97,
            sim_edge: 0.75,
            entity_seed: 0.55,
            pred_alias: 0.90,
            w_bm25: 1.0,
            w_vector: 2.0,
            w_graph: 1.5,
            w_temporal: 1.0,
            w_sempath: 0.25,
            rrf_k: 5.0,
            alpha: 0.5,
            eps: 1e-5,
            expired_penalty: 0.35,
            mmr_lambda: 0.9,
            half_life_days: 90.0,
            bm25_rel: 0.0,
            fts_prefix_min: 4,
            mem_seed_share: 0.6,
            mem_seed_top: 20,
            history_boost: 1.5,
            fact_entity_boost: 1.0,
            fact_memory_seed: 0.0,
            update_candidate: 0.75,
        }
    }
}

pub struct Engine {
    pub db: Db,
    pub emb: Arc<dyn Embedder>,
    pub cfg: Config,
    /// Fixed "now" for tests/eval; None = system clock.
    pub fixed_clock: Option<Clock>,
    pub snapshot_cache: SnapshotCache,
}

impl Engine {
    pub fn open(path: &Path, emb: Arc<dyn Embedder>) -> Result<Engine> {
        Ok(Engine {
            db: Db::open(path)?,
            emb,
            cfg: Config::default(),
            fixed_clock: None,
            snapshot_cache: Mutex::new(None),
        })
    }

    pub fn open_in_memory(emb: Arc<dyn Embedder>) -> Result<Engine> {
        Ok(Engine {
            db: Db::open_in_memory()?,
            emb,
            cfg: Config::default(),
            fixed_clock: None,
            snapshot_cache: Mutex::new(None),
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
    pub exclusive: Option<bool>,
    /// Explicit polarity; when absent, remember infers conservative negation.
    #[serde(default)]
    pub negated: Option<bool>,
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
    pub conflicts: Vec<Conflict>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<RelatedMemory>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hints: Vec<String>,
    /// The text phrase a valid_from was inferred from (no --when given).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when_inferred: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Conflict {
    pub relation_id: i64,
    pub fact: String,
    pub valid_from: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct RelatedMemory {
    pub memory_id: i64,
    pub cosine: f32,
    pub snippet: String,
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
    /// Ended by forgetting its source rather than by a world-time end.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub retracted: bool,
    pub polarity: i64,
    /// Set when >= 2 positive values of (source, predicate) are current.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_values: Option<usize>,
    /// The newer relation that ended this one, as (id, fact).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replaced_by: Option<(i64, String)>,
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
    /// The query's time window overlaps no live memory (results are unfiltered).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub window_empty: bool,
    /// The top item alone exceeds the budget.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub over_budget: bool,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub representative: Option<MemoryHit>,
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
        crate::remember::remember(&self.db, self.emb.as_ref(), &self.cfg, self.clock(), _inp)
    }

    /// Store many memories with exactly the semantics of calling `remember` in
    /// order (later items see earlier ones), but one embed call and one
    /// transaction per chunk of up to 256 inputs. A failing input is reported
    /// in its own slot and does not abort the rest of the chunk.
    pub fn remember_batch(&self, inputs: Vec<RememberInput>) -> Vec<Result<RememberOutput>> {
        crate::remember::remember_batch(
            &self.db,
            self.emb.as_ref(),
            &self.cfg,
            self.clock(),
            inputs,
        )
    }

    /// Retrieve (design §8).
    pub fn recall(&self, inp: &RecallInput) -> Result<RecallOutput> {
        crate::recall::recall(self, inp)
    }

    /// Profile + timeline of one entity, by name, alias or e<id>.
    pub fn entity(&self, name_or_id: &str, limit: usize) -> Result<EntityOutput> {
        crate::recall::entity(self, name_or_id, limit)
    }

    /// End validity of a memory (m<id>) or relation (r<id>) at `at` (default now).
    /// A bare number is ambiguous and rejected: it used to mean a memory, so an
    /// agent asking for `r12` silently ended `m12`.
    pub fn invalidate(&self, _id: &str, _at: Option<&str>) -> Result<ChangeOutput> {
        let raw = _id.trim();
        let now = self.clock().now;
        let at = _at
            .map(|x| {
                crate::temporal::parse_instant(x, &self.clock())
                    .ok_or_else(|| crate::err!("invalid at; use ISO date or unix seconds"))
            })
            .transpose()?
            .unwrap_or(now);
        self.db.write(|db| {
            if raw.starts_with('m') {
                let id = parse_id(raw, 'm')
                    .ok_or_else(|| crate::err!("invalid memory id '{raw}'; use m<id>"))?;
                let vf: i64 = db
                    .sql()
                    .query_row("SELECT valid_from FROM memories WHERE id=?1", [id], |r| {
                        r.get(0)
                    })
                    .map_err(|_| crate::err!("unknown memory m{id}"))?;
                if at < vf {
                    return Err(crate::err!("at precedes memory valid_from"));
                }
                let end: Option<i64> =
                    db.sql()
                        .query_row("SELECT valid_to FROM memories WHERE id=?1", [id], |r| {
                            r.get(0)
                        })?;
                if let Some(end) = end.filter(|e| *e <= at) {
                    let when = crate::temporal::fmt_date(end, &self.clock());
                    return Ok(ChangeOutput {
                        changed: vec![format!("m{id} already ended at {when}")],
                    });
                }
                db.sql()
                    .execute("UPDATE memories SET valid_to=?1 WHERE id=?2", [at, id])?;
                crate::remember::end_relations_losing_evidence(db, id, at)?;
                crate::kg::bump_gen(db)?;
                return Ok(ChangeOutput {
                    changed: vec![format!("m{id} valid_to={at}")],
                });
            }
            if raw.starts_with('r') {
                let id = parse_id(raw, 'r')
                    .ok_or_else(|| crate::err!("invalid relation id '{raw}'; use r<id>"))?;
                let vf: i64 = db
                    .sql()
                    .query_row("SELECT valid_from FROM relations WHERE id=?1", [id], |r| {
                        r.get(0)
                    })
                    .map_err(|_| crate::err!("unknown relation r{id}"))?;
                if at < vf {
                    return Err(crate::err!("at precedes relation valid_from"));
                }
                let end: Option<i64> = db.sql().query_row(
                    "SELECT valid_to FROM relations WHERE id=?1",
                    [id],
                    |r| r.get(0),
                )?;
                if let Some(end) = end.filter(|e| *e <= at) {
                    let when = crate::temporal::fmt_date(end, &self.clock());
                    return Ok(ChangeOutput {
                        changed: vec![format!("r{id} already ended at {when}")],
                    });
                }
                db.sql()
                    .execute("UPDATE relations SET valid_to=?1 WHERE id=?2", [at, id])?;
                crate::remember::end_memories_of_ended_facts(db, &[id])?;
                crate::kg::bump_gen(db)?;
                return Ok(ChangeOutput {
                    changed: vec![format!("r{id} valid_to={at}")],
                });
            }
            if !raw.is_empty() && raw.bytes().all(|b| b.is_ascii_digit()) {
                return Err(crate::err!(
                    "id '{raw}' is ambiguous; use m<id> for a memory or r<id> for a relation"
                ));
            }
            Err(crate::err!("invalid id '{_id}'; use m<id> or r<id>"))
        })
    }

    /// Retire a memory (soft) or delete it with vectors/mentions/evidence (hard).
    /// Relations this memory was the only live evidence for end with it.
    pub fn forget(&self, _id: &str, _hard: bool) -> Result<ChangeOutput> {
        let now = self.clock().now;
        self.db.write(|db| {
            let Some(id) = parse_id(_id, 'm') else {
                return Err(crate::err!("invalid memory id '{_id}'"));
            };
            let exists: bool = db.sql().query_row(
                "SELECT EXISTS(SELECT 1 FROM memories WHERE id=?1)",
                [id],
                |r| r.get(0),
            )?;
            if !exists {
                return Err(crate::err!("unknown memory m{id}"));
            }
            // Relations with no other live evidence stop being current.
            let losing = crate::remember::relations_losing_their_last_evidence(db, id)?;
            crate::remember::retract_relations(db, &losing, now)?;
            if _hard {
                // Relations whose only evidence was this memory disappear with
                // it, including their FTS/vector rows.
                let orphaned: Vec<i64> = {
                    let mut st = db.sql().prepare(
                        "SELECT r.id FROM relations r
                         JOIN relation_evidence e ON e.relation_id=r.id
                         WHERE e.memory_id=?1
                           AND (SELECT count(*) FROM relation_evidence e2
                                WHERE e2.relation_id=r.id)=1",
                    )?;
                    let rows = st.query_map([id], |r| r.get(0))?;
                    rows.collect::<rusqlite::Result<_>>()?
                };
                for rid in &orphaned {
                    db.sql()
                        .execute("DELETE FROM relations_fts WHERE rowid=?1", [rid])?;
                    db.sql()
                        .execute("DELETE FROM relations_vec WHERE rowid=?1", [rid])?;
                    db.sql()
                        .execute("DELETE FROM relations WHERE id=?1", [rid])?;
                }
                // A deleted memory can never be restored: drop dangling refs
                // (the rows that pointed at it stay history).
                db.sql().execute(
                    "UPDATE memories SET superseded_by=NULL WHERE superseded_by=?1",
                    [id],
                )?;
                db.sql()
                    .execute("DELETE FROM memories_vec WHERE rowid=?1", [id])?;
                // Cascades mentions and relation_evidence.
                db.sql().execute("DELETE FROM memories WHERE id=?1", [id])?;
                // Incremental projection: only this node and the edges of the
                // relations that went with it, never a full rebuild.
                crate::kg::remove_memory(db, id)?;
                for rid in &orphaned {
                    crate::kg::remove_relation(db, *rid)?;
                }
            } else {
                // Soft forget keeps the projection node: it is history, and
                // snapshot/validate exclude it from the live graph.
                db.sql()
                    .execute("UPDATE memories SET expired_at=?1 WHERE id=?2", [now, id])?;
                db.sql()
                    .execute("UPDATE memories_vec SET live=0 WHERE rowid=?1", [id])?;
            }
            crate::kg::bump_gen(db)?;
            Ok(ChangeOutput {
                changed: vec![format!(
                    "m{id} {}",
                    if _hard { "deleted" } else { "expired" }
                )],
            })
        })
    }

    /// Merge entity `merge` into `keep` (aliases, mentions, relations move).
    pub fn merge_entities(&self, _keep: &str, _merge: &str) -> Result<ChangeOutput> {
        let k = parse_entity(&self.db, _keep)?;
        let m = parse_entity(&self.db, _merge)?;
        if k == m {
            return Err(crate::err!("cannot merge an entity into itself"));
        }
        let now = self.clock().now;
        self.db.write(|db| {
            // Move similar edges to `keep`, dropping self pairs and keeping the
            // highest score per pair.
            let moved: Vec<(i64, i64, f32)> = {
                let mut st = db
                    .sql()
                    .prepare("SELECT a,b,score FROM similar WHERE a=?1 OR b=?1")?;
                let rows = st.query_map([m], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
                rows.collect::<rusqlite::Result<_>>()?
            };
            db.sql()
                .execute("DELETE FROM similar WHERE a=?1 OR b=?1", [m])?;
            for (a, b, score) in moved {
                let other = if a == m { b } else { a };
                if other == k || other == m {
                    continue;
                }
                let (lo, hi) = if other < k { (other, k) } else { (k, other) };
                db.sql().execute(
                    "INSERT INTO similar(a,b,score) VALUES (?1,?2,?3)
                     ON CONFLICT(a,b) DO UPDATE SET score=MAX(score, excluded.score)",
                    rusqlite::params![lo, hi, score],
                )?;
            }
            db.sql().execute(
                "UPDATE OR IGNORE mentions SET entity_id=?1 WHERE entity_id=?2",
                [k, m],
            )?;
            db.sql()
                .execute("DELETE FROM mentions WHERE entity_id=?1", [m])?;
            db.sql().execute(
                "UPDATE OR IGNORE entity_aliases SET entity_id=?1, origin='manual' WHERE entity_id=?2",
                [k, m],
            )?;
            db.sql()
                .execute("DELETE FROM entity_aliases WHERE entity_id=?1", [m])?;
            db.sql()
                .execute("UPDATE relations SET src=?1 WHERE src=?2", [k, m])?;
            db.sql()
                .execute("UPDATE relations SET dst=?1 WHERE dst=?2", [k, m])?;
            // Self-loops created by the merge (or already present) are noise.
            let loops: Vec<i64> = {
                let mut st = db
                    .sql()
                    .prepare("SELECT id FROM relations WHERE src=dst AND src=?1")?;
                let rows = st.query_map([k], |r| r.get::<_, i64>(0))?;
                rows.collect::<rusqlite::Result<_>>()?
            };
            for rid in &loops {
                db.sql()
                    .execute("DELETE FROM relations_fts WHERE rowid=?1", [rid])?;
                db.sql()
                    .execute("DELETE FROM relations_vec WHERE rowid=?1", [rid])?;
                db.sql().execute("DELETE FROM relations WHERE id=?1", [rid])?;
                crate::kg::remove_relation(db, *rid)?;
            }
            // Two entities that described the same fact become one relation,
            // and an exclusive predicate keeps at most one live object.
            merge_duplicate_relations(db)?;
            let invalidated = reapply_exclusivity(db, now)?;
            crate::remember::end_memories_of_ended_facts(db, &invalidated)?;
            let merged_count: i64 = db.sql().query_row(
                "SELECT mention_count FROM entities WHERE id=?1",
                [m],
                |r| r.get(0),
            )?;
            db.sql().execute(
                "UPDATE entities SET mention_count=mention_count+?1, updated_at=?2 WHERE id=?3",
                rusqlite::params![merged_count, now, k],
            )?;
            db.sql()
                .execute("DELETE FROM entities_fts WHERE rowid=?1", [m])?;
            db.sql()
                .execute("DELETE FROM entities_vec WHERE rowid=?1", [m])?;
            db.sql().execute("DELETE FROM entities WHERE id=?1", [m])?;
            crate::resolve::refresh_entity_fts(db, k)?;
            // Incremental projection: drop the merged node (with its edges) and
            // re-project only what the surviving entity owns.
            crate::kg::remove_entity(db, m)?;
            crate::kg::reproject_entity(db, k)?;
            crate::kg::bump_gen(db)?;
            Ok(ChangeOutput {
                changed: vec![format!("e{m} merged into e{k}")],
            })
        })
    }

    /// Add an alias name to an entity.
    pub fn add_alias(&self, _entity: &str, _alias: &str) -> Result<ChangeOutput> {
        let id = parse_entity(&self.db, _entity)?;
        let norm = crate::text::norm_name(_alias);
        if norm.is_empty() {
            return Err(crate::err!("alias must not be empty"));
        }
        self.db.write(|db| {
            let other: Option<i64> = db
                .sql()
                .query_row(
                    "SELECT entity_id FROM entity_aliases WHERE alias_norm=?1 AND entity_id<>?2 LIMIT 1",
                    rusqlite::params![norm, id],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(other) = other {
                return Err(crate::err!(
                    "alias '{}' names e{}; merge entities first",
                    _alias,
                    other
                ));
            }
            db.sql().execute(
                "INSERT OR IGNORE INTO entity_aliases(alias_norm,entity_id,origin) VALUES(?1,?2,'manual')",
                rusqlite::params![norm, id],
            )?;
            crate::resolve::refresh_entity_fts(db, id)?;
            crate::kg::bump_gen(db)?;
            Ok(ChangeOutput {
                changed: vec![format!("e{id} alias={norm}")],
            })
        })
    }

    /// Communities of the entity graph, ranked by relevance to `query` (or size).
    pub fn overview(&self, query: Option<&str>, k: usize) -> Result<OverviewOutput> {
        crate::recall::overview(self, query, k)
    }

    pub fn stats(&self) -> Result<Stats> {
        let c = self.db.sql();
        Ok(Stats {
            memories: c.query_row("SELECT count(*) FROM memories", [], |r| r.get(0))?,
            live_memories: c.query_row(
                "SELECT count(*) FROM memories WHERE expired_at IS NULL",
                [],
                |r| r.get(0),
            )?,
            entities: c.query_row("SELECT count(*) FROM entities", [], |r| r.get(0))?,
            relations: c.query_row("SELECT count(*) FROM relations", [], |r| r.get(0))?,
            live_relations: c.query_row(
                "SELECT count(*) FROM relations WHERE expired_at IS NULL",
                [],
                |r| r.get(0),
            )?,
            similar_edges: c.query_row("SELECT count(*) FROM similar", [], |r| r.get(0))?,
            predicates: c.query_row("SELECT count(*) FROM predicates", [], |r| r.get(0))?,
            db_bytes: c.query_row(
                "SELECT page_count*page_size FROM pragma_page_count(), pragma_page_size()",
                [],
                |r| r.get(0),
            )?,
            embedder: self.emb.name().into(),
            version: env!("CARGO_PKG_VERSION").into(),
        })
    }

    /// Rebuild the graphqlite projection from the relational tables.
    pub fn reindex(&self) -> Result<ChangeOutput> {
        self.db.write(|db| {
            crate::kg::rebuild_all(db)?;
            crate::kg::bump_gen(db)?;
            Ok(ChangeOutput {
                changed: vec!["projection rebuilt".into()],
            })
        })
    }

    /// Integrity problems (projection drift, orphan vectors, dangling refs); empty = healthy.
    pub fn validate(&self) -> Result<Vec<String>> {
        let c = self.db.sql();
        let mut out = Vec::new();
        let count =
            |sql: &str| -> Result<i64> { Ok(c.query_row(sql, [], |r| r.get::<_, i64>(0))?) };
        let cypher_count = |query: &str| -> Result<i64> {
            Ok(self
                .db
                .graph()
                .cypher(query)?
                .iter()
                .next()
                .and_then(|r| r.get::<i64>("c").ok())
                .unwrap_or(0))
        };
        let nodes = cypher_count("MATCH (n) RETURN count(n) AS c")?;
        let edges = cypher_count("MATCH (a)-[r]->(b) RETURN count(r) AS c")?;
        let memories = count("SELECT count(*) FROM memories")?;
        let entities = count("SELECT count(*) FROM entities")?;
        let mentions = count("SELECT count(*) FROM mentions")?;
        let similar = count("SELECT count(*) FROM similar")?;
        let relations = count("SELECT count(*) FROM relations")?;
        if nodes != memories + entities {
            out.push(format!(
                "projection nodes: {nodes}, relational: {}",
                memories + entities
            ));
        }
        if edges != mentions + similar + relations {
            out.push(format!(
                "projection edges: {edges}, relational: {}",
                mentions + similar + relations
            ));
        }
        if count("SELECT count(*) FROM memories_vec")? != memories {
            out.push("memory vectors do not match memories".into());
        }
        if count("SELECT count(*) FROM entities_vec")? != entities {
            out.push("entity vectors do not match entities".into());
        }
        let dangling_mentions = count(
            "SELECT count(*) FROM mentions m
             LEFT JOIN memories mem ON mem.id=m.memory_id
             LEFT JOIN entities e ON e.id=m.entity_id
             WHERE mem.id IS NULL OR e.id IS NULL",
        )?;
        if dangling_mentions != 0 {
            out.push(format!(
                "{dangling_mentions} mentions point at missing rows"
            ));
        }
        let dangling_evidence = count(
            "SELECT count(*) FROM relation_evidence ev
             LEFT JOIN relations r ON r.id=ev.relation_id
             LEFT JOIN memories m ON m.id=ev.memory_id
             WHERE r.id IS NULL OR m.id IS NULL",
        )?;
        if dangling_evidence != 0 {
            out.push(format!(
                "{dangling_evidence} relation_evidence rows point at missing rows"
            ));
        }
        // A current relation whose every evidence memory has ended is contradictory.
        let mut st = c.prepare(
            "SELECT r.id FROM relations r
             WHERE r.expired_at IS NULL AND r.valid_to IS NULL
               AND EXISTS (SELECT 1 FROM relation_evidence e WHERE e.relation_id=r.id)
               AND NOT EXISTS (
                 SELECT 1 FROM relation_evidence e JOIN memories m ON m.id=e.memory_id
                 WHERE e.relation_id=r.id AND m.expired_at IS NULL AND m.valid_to IS NULL)",
        )?;
        for rid in st.query_map([], |r| r.get::<_, i64>(0))? {
            out.push(format!(
                "relation r{} is current but every memory that states it has ended",
                rid?
            ));
        }
        Ok(out)
    }

    /// Read-only Cypher; rejects CREATE/MERGE/SET/DELETE/REMOVE/DROP.
    pub fn cypher(&self, _query: &str) -> Result<serde_json::Value> {
        let upper = _query.to_ascii_uppercase();
        for word in [
            "CREATE", "MERGE", "SET", "DELETE", "DETACH", "REMOVE", "DROP", "CALL",
        ] {
            if upper
                .split(|c: char| !c.is_ascii_alphabetic())
                .any(|x| x == word)
            {
                return Err(crate::err!("cypher is read-only; {word} is not allowed"));
            }
        }
        let result = self.db.graph().cypher(_query)?;
        let rows: Vec<serde_json::Value> = result
            .iter()
            .map(|r| {
                let mut obj = serde_json::Map::new();
                for col in r.columns() {
                    if let Some(v) = r.get_value(col) {
                        obj.insert(
                            col.clone(),
                            serde_json::to_value(v).unwrap_or(serde_json::Value::Null),
                        );
                    }
                }
                serde_json::Value::Object(obj)
            })
            .collect();
        Ok(serde_json::Value::Array(rows))
    }
}

impl Engine {
    pub fn check_embedder(&self) -> Result<()> {
        crate::remember::check_embedder(&self.db, self.emb.as_ref())
    }
}
/// Collapse live relations that became the same `(src, predicate, dst)` after
/// an entity merge: keep the earliest start, sum the evidence, move the
/// evidence rows and the projections of the rows that go away.
fn merge_duplicate_relations(db: &Db) -> Result<()> {
    let groups: Vec<(i64, i64, i64, i64)> = {
        let mut st = db.sql().prepare(
            "SELECT src,predicate_id,dst,polarity FROM relations
             WHERE expired_at IS NULL AND valid_to IS NULL
             GROUP BY src,predicate_id,dst,polarity HAVING count(*)>1",
        )?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    for (src, pid, dst, polarity) in groups {
        let rows: Vec<(i64, i64)> = {
            let mut st = db.sql().prepare(
                "SELECT id,evidence_count FROM relations
                 WHERE src=?1 AND predicate_id=?2 AND dst=?3
                   AND polarity=?4 AND expired_at IS NULL AND valid_to IS NULL
                 ORDER BY valid_from, id",
            )?;
            let rows = st.query_map(rusqlite::params![src, pid, dst, polarity], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let keep = rows[0].0;
        let mut total = rows[0].1;
        for (rid, count) in &rows[1..] {
            total += count;
            db.sql().execute(
                "INSERT OR IGNORE INTO relation_evidence(relation_id,memory_id)
                 SELECT ?1,memory_id FROM relation_evidence WHERE relation_id=?2",
                rusqlite::params![keep, rid],
            )?;
            db.sql()
                .execute("DELETE FROM relation_evidence WHERE relation_id=?1", [rid])?;
            db.sql()
                .execute("DELETE FROM relations_fts WHERE rowid=?1", [rid])?;
            db.sql()
                .execute("DELETE FROM relations_vec WHERE rowid=?1", [rid])?;
            db.sql()
                .execute("DELETE FROM relations WHERE id=?1", [rid])?;
            crate::kg::remove_relation(db, *rid)?;
        }
        db.sql().execute(
            "UPDATE relations SET evidence_count=?1, weight=?1 WHERE id=?2",
            rusqlite::params![total, keep],
        )?;
    }
    Ok(())
}

/// Re-apply exclusive invalidation after a merge: per `(src, exclusive
/// predicate)` the newest `valid_from` wins; older rows end where it starts.
fn reapply_exclusivity(db: &Db, now: i64) -> Result<Vec<i64>> {
    let groups: Vec<(i64, i64)> = {
        let mut st = db.sql().prepare(
            "SELECT r.src,r.predicate_id FROM relations r
             JOIN predicates p ON p.id=r.predicate_id
             WHERE p.exclusive<>0 AND r.expired_at IS NULL AND r.valid_to IS NULL
             GROUP BY r.src,r.predicate_id HAVING count(*)>1",
        )?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let mut invalidated = Vec::new();
    for (src, pid) in groups {
        let rows: Vec<(i64, i64)> = {
            let mut st = db.sql().prepare(
                "SELECT id,valid_from FROM relations
                 WHERE src=?1 AND predicate_id=?2
                   AND expired_at IS NULL AND valid_to IS NULL
                 ORDER BY valid_from DESC, id DESC",
            )?;
            let rows = st.query_map(rusqlite::params![src, pid], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let (_, newest) = rows[0];
        for (rid, from) in &rows[1..] {
            if *from == newest {
                // Equal start: the newer write wins outright.
                db.sql().execute(
                    "UPDATE relations SET valid_to=?1, expired_at=?2 WHERE id=?3",
                    rusqlite::params![newest, now, rid],
                )?;
            } else {
                db.sql().execute(
                    "UPDATE relations SET valid_to=?1 WHERE id=?2",
                    rusqlite::params![newest, rid],
                )?;
            }
            invalidated.push(*rid);
        }
    }
    Ok(invalidated)
}

fn parse_entity(db: &Db, s: &str) -> Result<i64> {
    if let Some(id) = parse_id(s, 'e') {
        return Ok(id);
    };
    let n = crate::text::norm_name(s);
    db.sql()
        .query_row(
            "SELECT entity_id FROM entity_aliases WHERE alias_norm=?1 LIMIT 1",
            [n],
            |r| r.get(0),
        )
        .map_err(|_| crate::err!("unknown entity '{s}'"))
}
