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

pub fn run_eval(fixtures: &Path, emb: Arc<dyn Embedder>, cfg: Config) -> Result<EvalReport> {
    let data: Fixture = serde_json::from_str(&std::fs::read_to_string(fixtures)?)?;
    let now = temporal::parse_instant(
        &data.now,
        &Clock {
            now: 0,
            tz_offset_secs: 0,
        },
    )
    .ok_or_else(|| crate::err!("invalid fixture now '{}'", data.now))?;
    let mut engine = Engine::open_in_memory(emb)?.with_clock(Clock {
        now,
        tz_offset_secs: 0,
    });
    engine.cfg = cfg;
    let start = Instant::now();
    let mut ids = HashMap::new();
    let mut pending = Vec::new();
    for memory in data.memories {
        let supersedes = memory.input.supersedes.clone();
        let mut input = memory.input;
        input.supersedes.clear();
        let old = engine.remember(input)?;
        ids.insert(memory.key, old.id);
        pending.push((old.id, supersedes));
    }
    for (id, supersedes) in pending {
        for key in supersedes {
            if let Some(old) = ids.get(&key) {
                engine.db.sql().execute(
                    "UPDATE memories SET expired_at=?1,superseded_by=?2,valid_to=COALESCE(valid_to,?1) WHERE id=?3",
                    rusqlite::params![now, id, old],
                )?;
                engine
                    .db
                    .sql()
                    .execute("UPDATE memories_vec SET live=0 WHERE rowid=?1", [old])?;
            }
        }
    }
    let ingestion_ms = start.elapsed().as_secs_f64() * 1000.0;
    let mut latencies = Vec::new();
    let mut recall_sum = 0.0;
    let mut mrr_sum = 0.0;
    let mut by_category: HashMap<String, (usize, f64, f64)> = HashMap::new();
    for query in data.queries {
        let started = Instant::now();
        let out = engine.recall(&RecallInput {
            query: query.q,
            k: query.k,
            as_of: query.as_of,
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
        let first_expected = expected
            .iter()
            .filter_map(|id| found.iter().position(|x| x == id))
            .min();
        let first_absent = absent
            .iter()
            .filter_map(|id| found.iter().position(|x| x == id))
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
            first_expected.map(|x| 1.0 / (x + 1) as f64).unwrap_or(0.0)
        };
        recall_sum += hit;
        mrr_sum += rr;
        let entry = by_category.entry(query.category).or_default();
        entry.0 += 1;
        entry.1 += hit;
        entry.2 += rr;
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
    })
}

pub fn render_report(report: &EvalReport) -> String {
    format!(
        "queries: {}\nrecall: {:.3}\nmrr: {:.3}\nlatency mean/p50/p95: {:.2}/{:.2}/{:.2} ms\ningestion: {:.2} ms\ncategories: {}\n",
        report.queries,
        report.recall,
        report.mrr,
        report.latency_mean_ms,
        report.latency_p50_ms,
        report.latency_p95_ms,
        report.ingestion_ms,
        serde_json::to_string(&report.category).unwrap_or_default()
    )
}

pub fn ablation(
    fixtures: &Path,
    emb: Arc<dyn Embedder>,
    base: Config,
) -> Vec<(String, EvalReport)> {
    let mut out = Vec::new();
    for name in ["bm25", "vector", "graph", "temporal"] {
        let mut cfg = base.clone();
        match name {
            "bm25" => cfg.w_bm25 = 0.0,
            "vector" => cfg.w_vector = 0.0,
            "graph" => cfg.w_graph = 0.0,
            _ => cfg.w_temporal = 0.0,
        }
        if let Ok(report) = run_eval(fixtures, Arc::clone(&emb), cfg) {
            out.push((name.to_string(), report));
        }
    }
    out
}
