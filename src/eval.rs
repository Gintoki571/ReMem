use crate::embed::Embedder;
use crate::engine::Engine;
use crate::engine::{Config, RecallInput, RecallOutput, RememberInput};
use crate::error::Result;
use crate::temporal::{self, Clock};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

/// Accuracy is judged on the context an agent would actually read: the top
/// `ACC_K` memories plus the fact sentences and entity cards returned with them.
pub const ACC_K: usize = 10;

#[derive(Clone, Debug, Default, Serialize)]
pub struct EvalReport {
    pub queries: usize,
    pub recall: f64,
    pub mrr: f64,
    /// Fraction of answer-bearing queries whose every `answers` string appears
    /// in the rendered top-`ACC_K` context. `None` when no query has answers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acc: Option<f64>,
    #[serde(skip_serializing_if = "is_zero")]
    pub answer_queries: usize,
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

fn is_zero(value: &usize) -> bool {
    *value == 0
}

#[derive(Clone, Debug, Serialize)]
pub struct QueryFailure {
    pub query: String,
    pub category: String,
    pub expected: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub absent: Vec<String>,
    /// Answer strings that did not appear in the rendered context.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
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
    /// Strings that must appear in the rendered top-`ACC_K` context.
    #[serde(default)]
    answers: Vec<String>,
    #[serde(default)]
    as_of: Option<String>,
    #[serde(default)]
    include_history: bool,
    #[serde(default)]
    k: Option<usize>,
}

/// Where the database for a run comes from and how much of the query set to use.
#[derive(Clone, Debug, Default)]
pub struct EvalOptions {
    pub limit_queries: Option<usize>,
    /// Keep the ingested database at this path (plus `<path>.keys.json`).
    pub save_db: Option<PathBuf>,
    /// Skip ingestion and evaluate against this database.
    pub load_db: Option<PathBuf>,
}

fn keys_path(db: &Path) -> PathBuf {
    PathBuf::from(format!("{}.keys.json", db.display()))
}

fn save_keys(db: &Path, ids: &HashMap<String, i64>) -> Result<()> {
    let path = keys_path(db);
    std::fs::write(&path, serde_json::to_vec(ids)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn load_keys(db: &Path) -> Result<Ids> {
    let path = keys_path(db);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        crate::err!(
            "--load-db needs the key map written by --save-db at {}: {e}",
            path.display()
        )
    })?;
    Ok(serde_json::from_str(&text)?)
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

fn open(now: i64, emb: Arc<dyn Embedder>, opts: &EvalOptions) -> Result<Engine> {
    let clock = Clock {
        now,
        tz_offset_secs: 0,
    };
    if let Some(path) = &opts.load_db {
        if !path.exists() {
            return Err(crate::err!("--load-db {} does not exist", path.display()));
        }
        return Ok(Engine::open(path, emb)?.with_clock(clock));
    }
    if let Some(path) = &opts.save_db {
        // An eval database is an output path: start from an empty file.
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        return Ok(Engine::open(path, emb)?.with_clock(clock));
    }
    Ok(Engine::open_in_memory(emb)?.with_clock(clock))
}

/// Fixture key -> memory id, and memory id -> fixture key.
type Ids = HashMap<String, i64>;
type Keys = HashMap<i64, String>;

/// Store every fixture memory once, through `remember_batch` (one embed call
/// and one transaction per chunk), reporting progress for large worlds.
/// Returns key -> id, id -> key and the elapsed milliseconds.
fn ingest(engine: &Engine, memories: Vec<MemoryFixture>) -> Result<(Ids, Keys, f64)> {
    let start = Instant::now();
    let total = memories.len();
    let mut ids: Ids = HashMap::new();
    let mut keys: Keys = HashMap::new();
    let mut pending: Vec<(String, RememberInput)> = Vec::new();
    for memory in memories {
        let mut input = memory.input;
        let unresolved = input.supersedes.iter().any(|key| !ids.contains_key(key));
        if unresolved && !pending.is_empty() {
            // The target is in the pending chunk: flush so its id exists.
            store(engine, &mut pending, &mut ids, &mut keys, total, start)?;
        }
        input.supersedes = input
            .supersedes
            .iter()
            .map(|key| {
                ids.get(key)
                    .copied()
                    .map(|id| format!("m{id}"))
                    .ok_or_else(|| crate::err!("unknown supersedes key '{key}'"))
            })
            .collect::<Result<Vec<_>>>()?;
        pending.push((memory.key, input));
        if pending.len() >= crate::remember::BATCH_SIZE {
            store(engine, &mut pending, &mut ids, &mut keys, total, start)?;
        }
    }
    store(engine, &mut pending, &mut ids, &mut keys, total, start)?;
    Ok((ids, keys, start.elapsed().as_secs_f64() * 1000.0))
}

fn store(
    engine: &Engine,
    pending: &mut Vec<(String, RememberInput)>,
    ids: &mut Ids,
    keys: &mut Keys,
    total: usize,
    start: Instant,
) -> Result<()> {
    if pending.is_empty() {
        return Ok(());
    }
    let names: Vec<String> = pending.iter().map(|(key, _)| key.clone()).collect();
    let inputs: Vec<RememberInput> = pending.drain(..).map(|(_, input)| input).collect();
    for (key, result) in names.into_iter().zip(engine.remember_batch(inputs)) {
        let out = result.map_err(|e| crate::err!("memory '{key}': {e}"))?;
        ids.insert(key.clone(), out.id);
        keys.insert(out.id, key);
    }
    let done = ids.len();
    if total >= 1000 && (done % 1000 < crate::remember::BATCH_SIZE || done == total) {
        eprintln!(
            "ingested {done}/{total} memories ({:.0}s)",
            start.elapsed().as_secs_f64()
        );
    }
    Ok(())
}

/// The text an agent sees for a recall result: memory contents, fact sentences
/// and entity cards, lowercased for case-insensitive answer matching.
fn context_text(out: &RecallOutput, k: usize) -> String {
    let mut text = String::new();
    for memory in out.memories.iter().take(k) {
        text.push_str(&memory.content);
        text.push('\n');
    }
    for fact in &out.facts {
        text.push_str(&fact.fact);
        text.push('\n');
        text.push_str(&fact.source);
        text.push(' ');
        text.push_str(&fact.target);
        text.push('\n');
    }
    for entity in &out.entities {
        text.push_str(&entity.name);
        text.push(' ');
        text.push_str(&entity.aliases.join(" "));
        text.push(' ');
        text.push_str(&entity.description);
        text.push('\n');
    }
    text.to_lowercase()
}

#[derive(Default)]
struct CategoryScore {
    queries: usize,
    recall: f64,
    mrr: f64,
    answer_queries: usize,
    acc: f64,
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
    let mut answer_queries = 0;
    let mut acc_hits = 0;
    let mut failures = Vec::new();
    let mut by_category: HashMap<String, CategoryScore> = HashMap::new();
    for query in queries {
        for key in query.expect.iter().chain(query.expect_absent.iter()) {
            if !ids.contains_key(key) {
                return Err(crate::err!("unknown evaluation memory key '{key}'"));
            }
        }
        // Answer queries look at one more context width than the recall cutoff,
        // so `recall` keeps its exact meaning (the top-k prefix is stable).
        let k = query.k.unwrap_or(8);
        let requested = if query.answers.is_empty() {
            query.k
        } else {
            Some(k.max(ACC_K))
        };
        let started = Instant::now();
        let out = engine.recall(&RecallInput {
            query: query.q.clone(),
            k: requested,
            as_of: query.as_of.clone(),
            include_history: query.include_history,
            ..Default::default()
        })?;
        latencies.push(started.elapsed().as_secs_f64() * 1000.0);
        let found: Vec<i64> = out.memories.iter().take(k).map(|m| m.id).collect();
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
        // acc@ACC_K: every answer string present in the rendered context.
        let mut missing: Vec<String> = Vec::new();
        if !query.answers.is_empty() {
            let context = context_text(&out, ACC_K);
            missing = query
                .answers
                .iter()
                .filter(|answer| !context.contains(&answer.to_lowercase()))
                .cloned()
                .collect();
            answer_queries += 1;
            if missing.is_empty() {
                acc_hits += 1;
            }
        }
        recall_sum += hit;
        mrr_sum += rr;
        let entry = by_category.entry(query.category.clone()).or_default();
        entry.queries += 1;
        entry.recall += hit;
        entry.mrr += rr;
        if !query.answers.is_empty() {
            entry.answer_queries += 1;
            if missing.is_empty() {
                entry.acc += 1.0;
            }
        }
        if violated || hit < 1.0 || rr < 1.0 || !missing.is_empty() {
            failures.push(QueryFailure {
                query: query.q.clone(),
                category: query.category.clone(),
                expected: query.expect.clone(),
                absent: query.expect_absent.clone(),
                missing,
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
    let n = latencies.len();
    let category = by_category
        .into_iter()
        .map(|(name, score)| {
            let queries = score.queries.max(1) as f64;
            let mut value = serde_json::json!({
                "queries": score.queries,
                "recall": score.recall / queries,
                "mrr": score.mrr / queries,
            });
            if score.answer_queries > 0 {
                value["answer_queries"] = serde_json::json!(score.answer_queries);
                value["acc"] = serde_json::json!(score.acc / score.answer_queries as f64);
            }
            (name, value)
        })
        .collect::<serde_json::Map<_, _>>();
    Ok(EvalReport {
        queries: latencies.len(),
        recall: if n == 0 { 0.0 } else { recall_sum / n as f64 },
        mrr: if n == 0 { 0.0 } else { mrr_sum / n as f64 },
        acc: if answer_queries == 0 {
            None
        } else {
            Some(acc_hits as f64 / answer_queries as f64)
        },
        answer_queries,
        category: serde_json::Value::Object(category),
        latency_mean_ms: if n == 0 {
            0.0
        } else {
            latencies.iter().sum::<f64>() / n as f64
        },
        latency_p50_ms: n.checked_sub(1).map(|_| latencies[n / 2]).unwrap_or(0.0),
        latency_p95_ms: n
            .checked_sub(1)
            .map(|_| latencies[((n * 95) / 100).min(n - 1)])
            .unwrap_or(0.0),
        ingestion_ms,
        ingested: ingested as usize,
        failures,
    })
}

/// Ingest (or load) once, then evaluate. `opts.save_db` keeps the database and
/// its key map on disk so later runs can skip ingestion with `load_db`.
pub fn run_eval(
    fixtures: &Path,
    emb: Arc<dyn Embedder>,
    cfg: Config,
    opts: &EvalOptions,
) -> Result<EvalReport> {
    let (data, now) = load(fixtures)?;
    let mut engine = open(now, emb, opts)?;
    engine.cfg = cfg;
    let queries = limited(data.queries, opts.limit_queries);
    let (ids, keys, ingestion_ms) = match &opts.load_db {
        Some(path) => {
            let ids = load_keys(path)?;
            let keys = ids.iter().map(|(key, id)| (*id, key.clone())).collect();
            (ids, keys, 0.0)
        }
        None => {
            let (ids, keys, ms) = ingest(&engine, data.memories)?;
            if let Some(path) = &opts.save_db {
                save_keys(path, &ids)?;
                eprintln!(
                    "database saved to {} ({} memories)",
                    path.display(),
                    keys.len()
                );
            }
            (ids, keys, ms)
        }
    };
    evaluate(&engine, &queries, &ids, &keys, ingestion_ms)
}

fn limited(mut queries: Vec<QueryFixture>, limit: Option<usize>) -> Vec<QueryFixture> {
    if let Some(limit) = limit {
        queries.truncate(limit);
    }
    queries
}

/// Ingest once, then evaluate every config against the same engine.
pub fn sweep(
    fixtures: &Path,
    emb: Arc<dyn Embedder>,
    base: Config,
    variants: Vec<(String, Config)>,
) -> Result<Vec<(String, EvalReport)>> {
    sweep_with(fixtures, emb, base, variants, &EvalOptions::default())
}

pub fn sweep_with(
    fixtures: &Path,
    emb: Arc<dyn Embedder>,
    base: Config,
    variants: Vec<(String, Config)>,
    opts: &EvalOptions,
) -> Result<Vec<(String, EvalReport)>> {
    let (data, now) = load(fixtures)?;
    let mut engine = open(now, emb, opts)?;
    engine.cfg = base;
    let queries = limited(data.queries, opts.limit_queries);
    let (ids, keys, ingestion_ms) = match &opts.load_db {
        Some(path) => {
            let ids = load_keys(path)?;
            let keys = ids.iter().map(|(key, id)| (*id, key.clone())).collect();
            (ids, keys, 0.0)
        }
        None => {
            let (ids, keys, ms) = ingest(&engine, data.memories)?;
            if let Some(path) = &opts.save_db {
                save_keys(path, &ids)?;
            }
            (ids, keys, ms)
        }
    };
    let mut out = Vec::with_capacity(variants.len());
    for (name, cfg) in variants {
        engine.cfg = cfg;
        *engine.snapshot_cache.lock().unwrap() = None;
        let report = evaluate(&engine, &queries, &ids, &keys, ingestion_ms)?;
        out.push((name, report));
    }
    Ok(out)
}

pub const CONFIG_KEYS: &[&str] = &[
    "w_bm25",
    "w_vector",
    "w_graph",
    "w_temporal",
    "w_sempath",
    "rrf_k",
    "alpha",
    "entity_seed",
    "expired_penalty",
    "mmr_lambda",
    "half_life_days",
    "bm25_rel",
    "fts_prefix_min",
    "mem_seed_share",
    "mem_seed_top",
    "history_boost",
    "fact_entity_boost",
    "fact_memory_seed",
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
        "w_sempath" => cfg.w_sempath = number(key, value)?,
        "rrf_k" => cfg.rrf_k = number(key, value)?,
        "alpha" => cfg.alpha = number(key, value)?,
        "entity_seed" => cfg.entity_seed = number(key, value)? as f32,
        "expired_penalty" => cfg.expired_penalty = number(key, value)?,
        "mmr_lambda" => cfg.mmr_lambda = number(key, value)?,
        "half_life_days" => cfg.half_life_days = number(key, value)?,
        "bm25_rel" => cfg.bm25_rel = number(key, value)?,
        "fts_prefix_min" => cfg.fts_prefix_min = number(key, value)? as usize,
        "mem_seed_share" => cfg.mem_seed_share = number(key, value)?,
        "mem_seed_top" => cfg.mem_seed_top = number(key, value)? as usize,
        "history_boost" => cfg.history_boost = number(key, value)?,
        "fact_entity_boost" => cfg.fact_entity_boost = number(key, value)?,
        "fact_memory_seed" => cfg.fact_memory_seed = number(key, value)?,
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
        "queries: {}\nrecall: {:.3}\nmrr: {:.3}\n",
        report.queries, report.recall, report.mrr
    );
    if let Some(acc) = report.acc {
        out.push_str(&format!(
            "acc@{ACC_K}: {acc:.3} ({} answer queries)\n",
            report.answer_queries
        ));
    }
    out.push_str(&format!(
        "latency mean/p50/p95: {:.2}/{:.2}/{:.2} ms\ningestion: {:.2} ms ({} memories)\ncategories: {}\n",
        report.latency_mean_ms,
        report.latency_p50_ms,
        report.latency_p95_ms,
        report.ingestion_ms,
        report.ingested,
        serde_json::to_string(&report.category).unwrap_or_default()
    ));
    if report.answer_queries > 0 {
        out.push_str(&category_table(report));
    }
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
                "  [{}] {}{}{} | expected {} | top {}\n",
                failure.category,
                failure.query,
                if failure.violated {
                    " (absent outranked expected)"
                } else {
                    ""
                },
                if failure.missing.is_empty() {
                    String::new()
                } else {
                    format!(" (missing {:?})", failure.missing)
                },
                ranks.join(" "),
                top.join(" ")
            ));
        }
    }
    out
}

/// Per-category acc@k / recall / MRR, one row per category, sorted by name.
pub fn category_table(report: &EvalReport) -> String {
    let Some(map) = report.category.as_object() else {
        return String::new();
    };
    let mut out = format!(
        "\n{:<22} {:>6} {:>8} {:>7} {:>7}\n",
        "category", "q", "acc@10", "recall", "mrr"
    );
    let mut names: Vec<&String> = map.keys().collect();
    names.sort();
    for name in names {
        let value = &map[name];
        let number = |key: &str| value.get(key).and_then(|v| v.as_f64());
        let acc = match number("acc") {
            Some(acc) => format!("{acc:.3}"),
            None => "-".into(),
        };
        out.push_str(&format!(
            "{:<22} {:>6} {:>8} {:>7.3} {:>7.3}\n",
            name,
            value.get("queries").and_then(|v| v.as_u64()).unwrap_or(0),
            acc,
            number("recall").unwrap_or(0.0),
            number("mrr").unwrap_or(0.0)
        ));
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
