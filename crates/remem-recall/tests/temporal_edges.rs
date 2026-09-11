//! Temporal proximity edges at remember() (Hindsight pattern): memories with
//! occurred_at within 24h of the new memory get a TEMPORAL_NEAR edge,
//! weight = max(0.3, 1.0 - gap_hours/24.0), cap 20 per memory,
//! provenance temporal. Skip when occurred_at is NULL. RED-first.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use remem_graph::EdgeProvenance;
use remem_recall::RecallEngine;
use remem_store::Store;
use remem_types::{MemoryItem, MemoryKind};

struct FakeEmbedder {
    map: HashMap<String, usize>,
}

impl FakeEmbedder {
    fn new(pairs: &[(&str, usize)]) -> Self {
        Self {
            map: pairs.iter().map(|(t, i)| (t.to_string(), *i)).collect(),
        }
    }
}

impl remem_recall::Embed for FakeEmbedder {
    fn embed(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        Ok(texts
            .iter()
            .map(|t| {
                let mut v = vec![0.0f32; 768];
                let idx = self.map.get(*t).copied().unwrap_or(767);
                v[idx] = 1.0;
                v
            })
            .collect())
    }
}

fn temp_db(tag: &str) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "remem-temporal-{tag}-{}-{n}.db",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&p);
    p
}

fn cleanup(path: &std::path::Path) {
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(PathBuf::from(format!("{}{}", path.display(), suffix)));
    }
}

fn engine(tag: &str, embed: FakeEmbedder) -> (RecallEngine, PathBuf) {
    let path = temp_db(tag);
    let store = Store::open(path.to_str().unwrap()).unwrap();
    let graph = remem_graph::Graph::open(&path).unwrap();
    (
        RecallEngine::new(store, Box::new(embed)).with_graph(graph),
        path,
    )
}

const HOUR: i64 = 3600;

fn event(content: &str, occurred_at: i64) -> MemoryItem {
    let mut m = MemoryItem::new(MemoryKind::Event, content.to_string());
    m.occurred_at = Some(occurred_at);
    m
}

fn temporal_edges(g: &remem_graph::Graph) -> Vec<(String, String)> {
    g.memory_edges()
        .unwrap()
        .into_iter()
        .filter(|e| e.rel == "TEMPORAL_NEAR")
        .map(|e| (e.from, e.to))
        .collect()
}

#[test]
fn temporal_edges_link_within_24h_only() {
    let t0 = 1_700_000_000;
    let (e, path) = engine(
        "win",
        FakeEmbedder::new(&[("alpha meeting notes", 0), ("beta deploy report", 1)]),
    );
    let m1 = e.remember(&event("alpha meeting notes", t0)).unwrap().0;
    let m2 = e
        .remember(&event("beta deploy report", t0 + 2 * HOUR))
        .unwrap()
        .0;
    let m3 = e
        .remember(&event("gamma unrelated probe", t0 + 30 * HOUR))
        .unwrap()
        .0;

    let edges = temporal_edges(e.graph().unwrap());
    // Directional: each new memory links back to earlier neighbours.
    assert!(
        edges.contains(&(m2.clone(), m1.clone())),
        "m2 -> m1 missing: {edges:?}"
    );
    assert!(
        !edges.iter().any(|(f, t)| t == &m3 || f == &m3),
        "30h-away memory must not be linked: {edges:?}"
    );
    assert_eq!(edges.len(), 1, "exactly one temporal edge: {edges:?}");

    // Provenance + weight: 2h gap -> 1.0 - 2/24 = 11/12.
    let me = e
        .graph()
        .unwrap()
        .memory_edges()
        .unwrap()
        .into_iter()
        .find(|e| e.rel == "TEMPORAL_NEAR")
        .unwrap();
    assert_eq!(me.provenance, EdgeProvenance::Temporal);
    let w: f64 = e
        .graph()
        .unwrap()
        .sqlite()
        .query_row(
            "SELECT p.value FROM edge_props_real p \
             JOIN property_keys k ON k.id = p.key_id WHERE k.key = 'weight' \
             AND p.edge_id = (SELECT id FROM edges WHERE type = 'TEMPORAL_NEAR')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!((w - (1.0 - 2.0 / 24.0)).abs() < 1e-9, "weight {w}");
    cleanup(&path);
}

#[test]
fn temporal_edges_skip_null_occurred_at() {
    let (e, path) = engine(
        "null",
        FakeEmbedder::new(&[("alpha timeless fact", 0), ("beta timed event", 1)]),
    );
    let t0 = 1_700_000_000;
    let timeless = e.remember(&{
        let mut m = MemoryItem::new(MemoryKind::Fact, "alpha timeless fact".into());
        m.occurred_at = None;
        m
    });
    let _ = timeless;
    let timed = e.remember(&event("beta timed event", t0)).unwrap().0;
    assert!(
        temporal_edges(e.graph().unwrap()).is_empty(),
        "NULL occurred_at must never link"
    );
    let _ = timed;
    cleanup(&path);
}

#[test]
fn temporal_edges_capped_at_20_per_memory() {
    let t0 = 1_700_000_000;
    let pairs: Vec<(String, usize)> = (0..25)
        .map(|i| (format!("event number {i} details"), i as usize))
        .collect();
    let pairs: Vec<(&str, usize)> = pairs.iter().map(|(s, i)| (s.as_str(), *i)).collect();
    let (e, path) = engine("cap", FakeEmbedder::new(&pairs));
    let mut ids = Vec::new();
    for i in 0..25 {
        let id = e
            .remember(&event(&format!("event number {i} details"), t0))
            .unwrap()
            .0;
        ids.push(id);
    }
    let edges = temporal_edges(e.graph().unwrap());
    for id in &ids {
        let n = edges.iter().filter(|(f, t)| f == id || t == id).count();
        assert!(n <= 20, "memory {id} has {n} temporal edges, cap is 20");
    }
    // Cap binds: the earliest memory receives links until it saturates at 20.
    assert_eq!(
        edges.iter().filter(|(_, t)| t == &ids[0]).count(),
        20,
        "earliest memory saturates at the 20-edge cap"
    );
    cleanup(&path);
}
