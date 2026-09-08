//! Engine integration tests: real Store + real Graph on one file, with a
//! fake one-hot basis-vector embedder.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use remem_recall::{rank, RecallEngine, Weights};
use remem_store::Store;
use remem_types::{MemoryItem, MemoryKind, RecallQuery};

const DAY: i64 = 86_400;

/// Maps exact texts to one-hot 768-dim vectors; unknown text gets index 767.
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
    let p = std::env::temp_dir().join(format!("remem-recall-{tag}-{}-{n}.db", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

fn item(content: &str) -> MemoryItem {
    MemoryItem::new(MemoryKind::Fact, content.to_string())
}

/// Engine with graph on one shared file, like the CLI does.
fn engine(tag: &str, embed: FakeEmbedder) -> (RecallEngine, PathBuf) {
    let path = temp_db(tag);
    let store = Store::open(path.to_str().unwrap()).unwrap();
    let graph = remem_graph::Graph::open(&path).unwrap();
    (
        RecallEngine::new(store, Box::new(embed)).with_graph(graph),
        path,
    )
}

fn cleanup(path: &std::path::Path) {
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(PathBuf::from(format!("{}{}", path.display(), suffix)));
    }
}

fn q(text: &str, k: usize) -> RecallQuery {
    RecallQuery {
        text: text.to_string(),
        k,
        ..Default::default()
    }
}

#[test]
fn remember_then_fts_recall() {
    let (e, path) = engine("fts", FakeEmbedder::new(&[]));
    e.remember(&item("deploy script is at scripts/deploy.mjs"))
        .unwrap();
    e.remember(&item("python venv lives in .venv")).unwrap();
    let hits = e.recall(&q("deploy script", 5)).unwrap();
    assert!(!hits.is_empty());
    assert!(hits[0].item.content.contains("deploy script"));
    assert!(hits[0].reasons.iter().any(|r| r.starts_with("fts#")));
    cleanup(&path);
}

#[test]
fn empty_store_and_empty_query_return_nothing() {
    let (e, path) = engine("empty", FakeEmbedder::new(&[]));
    assert!(e.recall(&q("anything", 5)).unwrap().is_empty());
    e.remember(&item("some fact")).unwrap();
    assert!(e.recall(&q("   ", 5)).unwrap().is_empty());
    cleanup(&path);
}

#[test]
fn vector_list_fuses_with_fts() {
    // b shares no query keywords but has the query's exact vector: it can
    // only arrive via vector#.
    let (e, path) = engine(
        "fuse",
        FakeEmbedder::new(&[
            ("ship the release", 0),
            ("push the artifact out", 0),
            ("unrelated banana", 1),
        ]),
    );
    e.remember(&item("ship the release")).unwrap();
    e.remember(&item("push the artifact out")).unwrap();
    e.remember(&item("unrelated banana")).unwrap();
    let hits = e.recall(&q("ship the release", 5)).unwrap();
    assert_eq!(hits[0].item.content, "ship the release"); // fts + vector
    assert!(hits[0].reasons.iter().any(|r| r.starts_with("fts#")));
    assert!(hits[0].reasons.iter().any(|r| r.starts_with("vector#")));
    let b = hits
        .iter()
        .find(|h| h.item.content == "push the artifact out")
        .unwrap();
    assert!(b.reasons.iter().any(|r| r.starts_with("vector#")));
    assert!(!b.reasons.iter().any(|r| r.starts_with("fts#")));
    cleanup(&path);
}

#[test]
fn appearing_in_both_lists_outranks_single_list() {
    // query is exact text of a; b is keyword-free but vector-identical;
    // c is keyword-free and vector-far.
    let (e, path) = engine(
        "both",
        FakeEmbedder::new(&[
            ("alpha keyword", 0),
            ("beta no tokens", 0),
            ("gamma other", 1),
        ]),
    );
    e.remember(&item("alpha keyword")).unwrap();
    e.remember(&item("beta no tokens")).unwrap();
    e.remember(&item("gamma other")).unwrap();
    let hits = e.recall(&q("alpha keyword", 5)).unwrap();
    assert_eq!(hits[0].item.content, "alpha keyword");
    assert!(hits[0].score > hits[1].score);
    cleanup(&path);
}

#[test]
fn importance_breaks_ties() {
    let (e, path) = engine("imp", FakeEmbedder::new(&[]));
    let mut low = item("same content text");
    low.importance = 0.1;
    let mut high = item("same content text");
    high.importance = 0.9;
    e.remember(&low).unwrap();
    e.remember(&high).unwrap();
    let hits = e.recall(&q("same content text", 5)).unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].item.importance, 0.9);
    assert!(hits[0].reasons.contains(&"important".to_string()));
    assert!(hits[0].score > hits[1].score);
    cleanup(&path);
}

#[test]
fn recency_breaks_ties() {
    let (e, path) = engine("rec", FakeEmbedder::new(&[]));
    let now = MemoryItem::now();
    let mut old = item("stale keyword fact");
    old.updated_at = now - 90 * DAY;
    old.created_at = old.updated_at;
    e.store().insert(&old).unwrap();
    let fresh = e.remember(&item("stale keyword fact")).unwrap();
    let hits = e.recall(&q("stale keyword fact", 5)).unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].item.id, fresh);
    assert!(hits[0].reasons.contains(&"recent".to_string()));
    assert!(!hits[1].reasons.contains(&"recent".to_string()));
    // 30d half-life: 90 days old -> 0.125
    let r = rank::recency_score(old.updated_at, 30.0, now);
    assert!((r - 0.125).abs() < 1e-6);
    cleanup(&path);
}

#[test]
fn kind_and_tag_filters() {
    let (e, path) = engine("filter", FakeEmbedder::new(&[]));
    let mut a = item("deploy the app");
    a.kind = MemoryKind::Decision;
    let mut b = item("deploy the app again");
    b.tags = vec!["prod".into()];
    let aid = e.remember(&a).unwrap();
    let bid = e.remember(&b).unwrap();
    let hits = e
        .recall(&RecallQuery {
            text: "deploy".into(),
            k: 5,
            kinds: Some(vec![MemoryKind::Decision]),
            tags: None,
            agent_id: None,
            session_id: None,
        })
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].item.id, aid);
    let hits = e
        .recall(&RecallQuery {
            text: "deploy".into(),
            k: 5,
            kinds: None,
            tags: Some(vec!["prod".into()]),
            agent_id: None,
            session_id: None,
        })
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].item.id, bid);
    cleanup(&path);
}

#[test]
fn graph_expansion_surfaces_linked_memory() {
    // Query hits only the hub; the satellite shares no tokens and is
    // vector-far, but the memory link pulls it in with a graph# reason.
    let (e, path) = engine(
        "graph",
        FakeEmbedder::new(&[("database tuning notes", 0), ("query", 0)]),
    );
    let hub_id = e.remember(&item("database tuning notes")).unwrap();
    let sat = item("completely different pastry");
    let sat_id = sat.id.clone();
    e.remember(&sat).unwrap();
    e.link(&hub_id, &sat_id, None).unwrap();
    let hits = e.recall(&q("database tuning notes", 5)).unwrap();
    assert_eq!(hits[0].item.id, hub_id);
    let sat_hit = hits
        .iter()
        .find(|h| h.item.id == sat_id)
        .expect("linked memory recalled");
    assert!(sat_hit.reasons.iter().any(|r| r.starts_with("graph#")));
    cleanup(&path);
}

#[test]
fn hub_edges_do_not_pollute_graph_expansion() {
    // Agent/session hub nodes must never surface as hits or graph# reasons.
    let (e, path) = engine("hub", FakeEmbedder::new(&[("tagged memory with agent", 0)]));
    let mut m = item("tagged memory with agent");
    m.agent_id = "agent-a".into();
    m.session_id = "sess-1".into();
    let id = e.remember(&m).unwrap();
    e.remember(&item("unrelated banana bread")).unwrap();
    let hits = e.recall(&q("tagged memory with agent", 5)).unwrap();
    assert_eq!(hits[0].item.id, id);
    assert!(hits
        .iter()
        .all(|h| h.item.id != "agent-a" && h.item.id != "sess-1"));
    assert!(hits
        .iter()
        .all(|h| h.reasons.iter().all(|r| !r.starts_with("graph#"))));
    cleanup(&path);
}

#[test]
fn k_truncates_hits() {
    let (e, path) = engine("k", FakeEmbedder::new(&[]));
    for i in 0..10 {
        e.remember(&item(&format!("shared keyword entry {i}")))
            .unwrap();
    }
    let hits = e.recall(&q("shared keyword entry", 3)).unwrap();
    assert_eq!(hits.len(), 3);
    cleanup(&path);
}

#[test]
fn forget_removes_from_recall_and_stats() {
    let (e, path) = engine("forget", FakeEmbedder::new(&[]));
    let id = e.remember(&item("temporary keyword note")).unwrap();
    e.remember(&item("keep this keyword note")).unwrap();
    assert_eq!(e.stats().unwrap()["memories"], 2);
    assert_eq!(e.stats().unwrap()["graph"]["nodes"], 2);
    e.forget(&id).unwrap();
    let hits = e.recall(&q("temporary keyword note", 5)).unwrap();
    assert!(!hits.iter().any(|h| h.item.id == id));
    assert_eq!(e.stats().unwrap()["memories"], 1);
    assert_eq!(e.stats().unwrap()["graph"]["nodes"], 1);
    cleanup(&path);
}

#[test]
fn weights_tune_fts_vs_vector() {
    // a matches the query keyword but is vector-far; b matches no keyword but
    // has the query's exact vector. Default weights: a wins (two lists). With
    // fts zeroed the ranking is pure vector, so b wins.
    let (e, path) = engine(
        "weights",
        FakeEmbedder::new(&[("keyword", 0), ("keyword only", 1)]),
    );
    e.remember(&item("keyword only")).unwrap(); // a: fts#1 + vector#2
    e.remember(&item("pastry content")).unwrap(); // b: vector#1 only
    let hits = e.recall(&q("keyword", 5)).unwrap();
    assert_eq!(hits[0].item.content, "keyword only");
    let b = hits
        .iter()
        .find(|h| h.item.content == "pastry content")
        .unwrap();
    assert!(b.reasons.iter().any(|r| r.starts_with("vector#")));
    assert!(!b.reasons.iter().any(|r| r.starts_with("fts#")));
    let e = e.with_weights(Weights {
        fts: 0.0,
        ..Default::default()
    });
    let hits = e.recall(&q("keyword", 5)).unwrap();
    assert_eq!(hits[0].item.content, "pastry content");
    cleanup(&path);
}
