use crate::embed::Embedder;
use crate::engine::Engine;
use crate::engine::{Config, RecallInput, RememberInput};
use crate::error::Result;
use crate::temporal::{self, Clock};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone, Debug, Default, Serialize)]
pub struct EvalReport {
    pub queries: usize,
    pub recall: f64,
    pub mrr: f64,
    pub category: serde_json::Value,
    pub latency_mean_ms: f64,
    pub latency_p50_ms: f64,
    pub latency_p95_ms: f64,
    pub ingestion_ms: f64,
    /// Memories in the database after ingestion (one ingestion per sweep).
    pub ingested: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub failures: Vec<QueryFailure>,
}

#[derive(Clone, Debug, Serialize)]
pub struct QueryFailure {
    pub query: String,
    pub category: String,
    pub expected: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub absent: Vec<String>,
    /// 1-based rank per expected key, in the same order (None = not returned).
    pub ranks: Vec<Option<usize>>,
    pub top: Vec<FailureHit>,
    /// An `expect_absent` memory outranked the expected ones.
    pub violated: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct FailureHit {
    pub key: String,
    pub reasons: Vec<String>,
    pub score: f64,
}

#[derive(Deserialize)]
struct Fixture {
    now: String,
    #[serde(default)]
    memories: Vec<MemoryFixture>,
    queries: Vec<QueryFixture>,
}

#[derive(Deserialize)]
struct MemoryFixture {
    key: String,
    #[serde(flatten)]
    input: RememberInput,
}

#[derive(Deserialize)]
struct QueryFixture {
    q: String,
    category: String,
    expect: Vec<String>,
    #[serde(default)]
    expect_absent: Vec<String>,
    #[serde(default)]
    as_of: Option<String>,
    #[serde(default)]
    include_history: bool,
    #[serde(default)]
    k: Option<usize>,
}

fn load(fixtures: &Path) -> Result<(Fixture, i64)> {
    let data: Fixture = serde_json::from_str(&std::fs::read_to_string(fixtures)?)?;
    let now = temporal::parse_instant(
        &data.now,
        &Clock {
            now: 0,
            tz_offset_secs: 0,
        },
    )
    .ok_or_else(|| crate::err!("invalid fixture now '{}'", data.now))?;
    Ok((data, now))
}

fn open(now: i64, emb: Arc<dyn Embedder>) -> Result<Engine> {
    Ok(Engine::open_in_memory(emb)?.with_clock(Clock {
        now,
        tz_offset_secs: 0,
    }))
}

/// Fixture key -> memory id, and memory id -> fixture key.
type Ids = HashMap<String, i64>;
type Keys = HashMap<i64, String>;

/// Store every fixture memory once. Returns key -> id, id -> key and the
/// elapsed milliseconds.
fn ingest(engine: &Engine, memories: Vec<MemoryFixture>) -> Result<(Ids, Keys, f64)> {
    let start = Instant::now();
    let mut ids = HashMap::new();
    let mut keys = HashMap::new();
    let mut pending = Vec::new();
    for memory in memories {
        let supersedes = memory.input.supersedes.clone();
        let mut input = memory.input;
        input.supersedes.clear();
        let out = engine.remember(input)?;
        ids.insert(memory.key.clone(), out.id);
        keys.insert(out.id, memory.key);
        pending.push((out.id, supersedes));
    }
    for (id, supersedes) in pending {
        for key in supersedes {
            if let Some(old) = ids.get(&key) {
                engine.db.sql().execute(
                    "UPDATE memories SET expired_at=?1,superseded_by=?2,valid_to=COALESCE(valid_to,?1) WHERE id=?3",
                    rusqlite::params![engine.clock().now, id, old],
                )?;
                engine
                    .db
                    .sql()
                    .execute("UPDATE memories_vec SET live=0 WHERE rowid=?1", [old])?;
            }
        }
    }
    Ok((ids, keys, start.elapsed().as_secs_f64() * 1000.0))
}

fn evaluate(
    engine: &Engine,
    queries: &[QueryFixture],
    ids: &HashMap<String, i64>,
    keys: &HashMap<i64, String>,
    ingestion_ms: f64,
) -> Result<EvalReport> {
    let ingested: i64 = engine
        .db
        .sql()
        .query_row("SELECT count(*) FROM memories", [], |r| r.get(0))?;
    let mut latencies = Vec::new();
    let mut recall_sum = 0.0;
    let mut mrr_sum = 0.0;
    let mut failures = Vec::new();
    let mut by_category: HashMap<String, (usize, f64, f64)> = HashMap::new();
    for query in queries {
        let started = Instant::now();
        let out = engine.recall(&RecallInput {
            query: query.q.clone(),
            k: query.k,
            as_of: query.as_of.clone(),
            include_history: query.include_history,
            ..Default::default()
        })?;
        latencies.push(started.elapsed().as_secs_f64() * 1000.0);
        let found: Vec<i64> = out.memories.iter().map(|m| m.id).collect();
        let expected: Vec<i64> = query
            .expect
            .iter()
            .filter_map(|key| ids.get(key).copied())
            .collect();
        let absent: Vec<i64> = query
            .expect_absent
            .iter()
            .filter_map(|key| ids.get(key).copied())
            .collect();
        let ranks: Vec<Option<usize>> = expected
            .iter()
            .map(|id| found.iter().position(|x| x == id).map(|p| p + 1))
            .collect();
        let first_expected = ranks.iter().flatten().copied().min();
        let first_absent = absent
            .iter()
            .filter_map(|id| found.iter().position(|x| x == id))
            .map(|p| p + 1)
            .min();
        let violated = first_absent.is_some_and(|a| first_expected.is_none_or(|e| a <= e));
        let hit = if violated {
            0.0
        } else {
            expected.iter().filter(|id| found.contains(id)).count() as f64
                / expected.len().max(1) as f64
        };
        let rr = if violated {
            0.0
        } else {
            first_expected.map(|x| 1.0 / x as f64).unwrap_or(0.0)
        };
        recall_sum += hit;
        mrr_sum += rr;
        let entry = by_category.entry(query.category.clone()).or_default();
        entry.0 += 1;
        entry.1 += hit;
        entry.2 += rr;
        if violated || hit < 1.0 || rr < 1.0 {
            failures.push(QueryFailure {
                query: query.q.clone(),
                category: query.category.clone(),
                expected: query.expect.clone(),
                absent: query.expect_absent.clone(),
                ranks,
                top: out
                    .memories
                    .iter()
                    .map(|m| FailureHit {
                        key: keys
                            .get(&m.id)
                            .cloned()
                            .unwrap_or_else(|| format!("m{}", m.id)),
                        reasons: m.reasons.clone(),
                        score: m.score,
                    })
                    .collect(),
                violated,
            });
        }
    }
    latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = latencies.len().max(1);
    let category = by_category
        .into_iter()
        .map(|(name, (count, recall, mrr))| {
            (name, serde_json::json!({"queries":count,"recall":recall/count.max(1) as f64,"mrr":mrr/count.max(1) as f64}))
        })
        .collect::<serde_json::Map<_, _>>();
    Ok(EvalReport {
        queries: latencies.len(),
        recall: recall_sum / n as f64,
        mrr: mrr_sum / n as f64,
        category: serde_json::Value::Object(category),
        latency_mean_ms: latencies.iter().sum::<f64>() / n as f64,
        latency_p50_ms: latencies[(n / 2).min(n - 1)],
        latency_p95_ms: latencies[((n * 95) / 100).min(n - 1)],
        ingestion_ms,
        ingested: ingested as usize,
        failures,
    })
}

pub fn run_eval(fixtures: &Path, emb: Arc<dyn Embedder>, cfg: Config) -> Result<EvalReport> {
    let (data, now) = load(fixtures)?;
    let mut engine = open(now, emb)?;
    engine.cfg = cfg;
    let (ids, keys, ingestion_ms) = ingest(&engine, data.memories)?;
    evaluate(&engine, &data.queries, &ids, &keys, ingestion_ms)
}

/// Ingest once, then evaluate every config against the same engine.
pub fn sweep(
    fixtures: &Path,
    emb: Arc<dyn Embedder>,
    base: Config,
    variants: Vec<(String, Config)>,
) -> Result<Vec<(String, EvalReport)>> {
    let (data, now) = load(fixtures)?;
    let mut engine = open(now, emb)?;
    engine.cfg = base;
    let (ids, keys, ingestion_ms) = ingest(&engine, data.memories)?;
    let mut out = Vec::with_capacity(variants.len());
    for (name, cfg) in variants {
        engine.cfg = cfg;
        *engine.snapshot_cache.lock().unwrap() = None;
        let report = evaluate(&engine, &data.queries, &ids, &keys, ingestion_ms)?;
        out.push((name, report));
    }
    Ok(out)
}

pub const CONFIG_KEYS: &[&str] = &[
    "w_bm25",
    "w_vector",
    "w_graph",
    "w_temporal",
    "rrf_k",
    "alpha",
    "entity_seed",
    "expired_penalty",
    "mmr_lambda",
    "half_life_days",
    "bm25_rel",
    "fts_prefix_min",
];

fn number(key: &str, value: &str) -> Result<f64> {
    value
        .parse::<f64>()
        .map_err(|_| crate::err!("invalid number '{value}' for {key}"))
}

/// Override one `Config` field by name (`--set key=value`).
pub fn set_config(cfg: &mut Config, key: &str, value: &str) -> Result<()> {
    match key {
        "w_bm25" => cfg.w_bm25 = number(key, value)?,
        "w_vector" => cfg.w_vector = number(key, value)?,
        "w_graph" => cfg.w_graph = number(key, value)?,
        "w_temporal" => cfg.w_temporal = number(key, value)?,
        "rrf_k" => cfg.rrf_k = number(key, value)?,
        "alpha" => cfg.alpha = number(key, value)?,
        "entity_seed" => cfg.entity_seed = number(key, value)? as f32,
        "expired_penalty" => cfg.expired_penalty = number(key, value)?,
        "mmr_lambda" => cfg.mmr_lambda = number(key, value)?,
        "half_life_days" => cfg.half_life_days = number(key, value)?,
        "bm25_rel" => cfg.bm25_rel = number(key, value)?,
        "fts_prefix_min" => cfg.fts_prefix_min = number(key, value)? as usize,
        other => {
            return Err(crate::err!(
                "unknown config key '{other}'; valid: {}",
                CONFIG_KEYS.join(", ")
            ))
        }
    }
    Ok(())
}

pub fn render_report(report: &EvalReport, show_failures: bool) -> String {
    let mut out = format!(
        "queries: {}\nrecall: {:.3}\nmrr: {:.3}\nlatency mean/p50/p95: {:.2}/{:.2}/{:.2} ms\ningestion: {:.2} ms ({} memories)\ncategories: {}\n",
        report.queries,
        report.recall,
        report.mrr,
        report.latency_mean_ms,
        report.latency_p50_ms,
        report.latency_p95_ms,
        report.ingestion_ms,
        report.ingested,
        serde_json::to_string(&report.category).unwrap_or_default()
    );
    if show_failures {
        out.push_str(&format!("failures: {}\n", report.failures.len()));
        for failure in &report.failures {
            let ranks: Vec<String> = failure
                .expected
                .iter()
                .zip(&failure.ranks)
                .map(|(key, rank)| match rank {
                    Some(r) => format!("{key}@{r}"),
                    None => format!("{key}@-"),
                })
                .collect();
            let top: Vec<String> = failure
                .top
                .iter()
                .map(|hit| format!("{}({})", hit.key, hit.reasons.join(",")))
                .collect();
            out.push_str(&format!(
                "  [{}] {}{} | expected {} | top {}\n",
                failure.category,
                failure.query,
                if failure.violated {
                    " (absent outranked expected)"
                } else {
                    ""
                },
                ranks.join(" "),
                top.join(" ")
            ));
        }
    }
    out
}

fn category(report: &EvalReport, name: &str) -> (f64, f64) {
    let value = report.category.get(name);
    (
        value
            .and_then(|v| v.get("recall"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0),
        value
            .and_then(|v| v.get("mrr"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0),
    )
}

/// Compact one-line-per-variant table for `--sweep`.
pub fn sweep_table(rows: &[(String, EvalReport)]) -> String {
    let mut out = format!(
        "{:<22} {:>7} {:>7} {:>7} {:>8} {:>7} {:>8} {:>8} {:>7}\n",
        "variant", "recall", "mrr", "multi", "multiMRR", "syn", "synMRR", "histMRR", "agg"
    );
    for (name, report) in rows {
        let (multi, multi_mrr) = category(report, "multi-hop");
        let (syn, syn_mrr) = category(report, "synonym");
        let (_, hist_mrr) = category(report, "history");
        let (agg, _) = category(report, "aggregation");
        out.push_str(&format!(
            "{:<22} {:>7.3} {:>7.3} {:>7.3} {:>8.3} {:>7.3} {:>8.3} {:>8.3} {:>7.3}\n",
            name, report.recall, report.mrr, multi, multi_mrr, syn, syn_mrr, hist_mrr, agg
        ));
    }
    out
}

/// Channel ablations as sweep variants (one ingestion covers all of them).
pub fn ablation_variants(base: &Config) -> Vec<(String, Config)> {
    let mut out = Vec::new();
    for name in ["bm25", "vector", "graph", "temporal"] {
        let mut cfg = base.clone();
        match name {
            "bm25" => cfg.w_bm25 = 0.0,
            "vector" => cfg.w_vector = 0.0,
            "graph" => cfg.w_graph = 0.0,
            _ => cfg.w_temporal = 0.0,
        }
        out.push((name.to_string(), cfg));
    }
    out
}

pub fn ablation(
    fixtures: &Path,
    emb: Arc<dyn Embedder>,
    base: Config,
) -> Result<Vec<(String, EvalReport)>> {
    let variants = ablation_variants(&base);
    sweep(fixtures, emb, base, variants)
}
