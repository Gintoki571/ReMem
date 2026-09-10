//! Lane C (issue #9): cosine abstention floor, hub-edge exclusion, recency
//! knobs, future-occurred-at clamp. Fakes only, no CLI wiring.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use remem_recall::{rank, Embed, RecallEngine, Weights};
use remem_store::Store;
use remem_types::{MemoryItem, MemoryKind, RecallQuery};

const DAY: i64 = 86_400;

/// Maps exact texts to one-hot 768-dim vectors; unknown text gets index 767.
/// The floor tests need real cosine geometry, so known texts pair up.
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

impl Embed for FakeEmbedder {
    fn embed(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        Ok(texts
            .iter()
            .map(|t| {
                let mut v = vec![0.0f32; 768];
                v[self.map.get(*t).copied().unwrap_or(767)] = 1.0;
                v
            })
            .collect())
    }
}

fn temp_db(tag: &str) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!("remem-lane-c-{tag}-{}-{n}.db", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

fn cleanup(path: &std::path::Path) {
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(PathBuf::from(format!("{}{}", path.display(), suffix)));
    }
}

fn item(content: &str) -> MemoryItem {
    MemoryItem::new(MemoryKind::Fact, content.to_string())
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

fn q(text: &str, k: usize) -> RecallQuery {
    RecallQuery {
        text: text.to_string(),
        k,
        ..Default::default()
    }
}

/// Two orthogonal memories and a query for the first: a, b, query.
fn orthogonal() -> FakeEmbedder {
    FakeEmbedder::new(&[("redis sharding plan", 0), ("kubernetes rollout notes", 1)])
}

#[test]
fn default_cosine_floor_value() {
    // 0.63: least-bad point on the fixtures. Junk cosine 0.5529..0.7018
    // overlaps the worst answerable 0.6461, so NO value separates them;
    // 0.63 keeps the 35 answerable and drops the most junk.
    assert_eq!(rank::DEFAULT_COSINE_FLOOR, 0.63);
}

#[test]
fn floor_drops_orthogonal_query_as_junk() {
    // Query embeds to basis 2; every stored cosine is exactly 0: far below
    // 0.63, so the floor abstains instead of returning an arbitrary hit.
    let embed = FakeEmbedder::new(&[
        ("redis sharding plan", 0),
        ("kubernetes rollout notes", 1),
        ("zig parser combinator", 2),
    ]);
    let (e, path) = engine("abstain", embed);
    e.remember(&item("redis sharding plan")).unwrap();
    e.remember(&item("kubernetes rollout notes")).unwrap();
    let hits = e.recall(&q("zig parser combinator", 5)).unwrap();
    assert!(hits.is_empty(), "zero-cosine query must abstain");
    cleanup(&path);
}

#[test]
fn floor_keeps_exact_match_query() {
    let (e, path) = engine("exact", orthogonal());
    e.remember(&item("redis sharding plan")).unwrap();
    e.remember(&item("kubernetes rollout notes")).unwrap();
    let hits = e.recall(&q("redis sharding plan", 5)).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].item.content, "redis sharding plan");
    cleanup(&path);
}

#[test]
fn floor_returns_empty_on_real_but_uncorrelated_query() {
    // Semi-realistic query sharing no embedder basis with the corpus. FTS has
    // no lexical anchor either. The floor answers "no" where pre-floor recall
    // would return the highest-of-junk hit.
    let embed = FakeEmbedder::new(&[
        ("quarterly revenue report", 0),
        ("onboarding checklist doc", 1),
        ("quarterly earnings summary", 0),
    ]);
    let (e, path) = engine("uncorrelated", embed);
    e.remember(&item("quarterly revenue report")).unwrap();
    e.remember(&item("onboarding checklist doc")).unwrap();
    let hits = e.recall(&q("kite surfing lessons", 5)).unwrap();
    assert!(hits.is_empty());
    cleanup(&path);
}

#[test]
fn agent_hub_edges_excluded_from_graph_expansion() {
    // agent-a is a hub node linked to the seed memory; it must never surface
    // as a hit or carry a graph# reason.
    let (e, path) = engine("hub-agent", orthogonal());
    let mut m = item("redis sharding plan");
    m.agent_id = "agent-a".into();
    let id = e.remember(&m).unwrap().0;
    e.remember(&item("kubernetes rollout notes")).unwrap();
    let hits = e.recall(&q("redis sharding plan", 5)).unwrap();
    assert!(hits.iter().all(|h| h.item.id != "agent-a"));
    assert!(hits
        .iter()
        .all(|h| h.reasons.iter().all(|r| !r.starts_with("graph#"))));
    let _ = id;
    cleanup(&path);
}

#[test]
fn session_hub_edges_excluded_from_graph_expansion() {
    let (e, path) = engine("hub-session", orthogonal());
    let mut m = item("redis sharding plan");
    m.session_id = "sess-7".into();
    e.remember(&m).unwrap();
    e.remember(&item("kubernetes rollout notes")).unwrap();
    let hits = e.recall(&q("redis sharding plan", 5)).unwrap();
    assert!(hits.iter().all(|h| h.item.id != "sess-7"));
    assert!(hits
        .iter()
        .all(|h| h.reasons.iter().all(|r| !r.starts_with("graph#"))));
    cleanup(&path);
}

#[test]
fn memory_to_memory_links_still_expand() {
    // Positive control: a deliberate Memory->Memory edge survives the hub
    // filter and surfaces the linked memory with a graph# reason.
    let (e, path) = engine("mem-link", orthogonal());
    let a = e.remember(&item("redis sharding plan")).unwrap().0;
    let b = e.remember(&item("kubernetes rollout notes")).unwrap().0;
    e.link(&a, &b, None).unwrap();
    // Query matches only a lexically/vector-wise; b must arrive via graph#.
    let hits = e.recall(&q("redis sharding", 5)).unwrap();
    let b_hit = hits
        .iter()
        .find(|h| h.item.id == b)
        .expect("linked memory recalled");
    assert!(b_hit.reasons.iter().any(|r| r.starts_with("graph#")));
    cleanup(&path);
}

#[test]
fn recency_off_skips_recent_reason() {
    let (e, path) = engine("recency-off", orthogonal());
    e.remember(&item("redis sharding plan")).unwrap();
    let hits = e
        .with_recency_off()
        .recall(&q("redis sharding plan", 5))
        .unwrap();
    assert!(!hits.is_empty());
    assert!(hits
        .iter()
        .all(|h| !h.reasons.contains(&"recent".to_string())));
    cleanup(&path);
}

#[test]
fn half_life_days_knob_changes_recency_weighting() {
    let (e, path) = engine("half-life", orthogonal());
    let mut old = item("redis sharding plan");
    old.created_at = MemoryItem::now() - 60 * DAY;
    e.remember(&old).unwrap();
    let fresh = e
        .with_half_life_days(1.0)
        .recall(&q("redis sharding plan", 5))
        .unwrap();
    let flat = e
        .with_half_life_days(30.0)
        .recall(&q("redis sharding plan", 5))
        .unwrap();
    assert!(!fresh.is_empty() && !flat.is_empty());
    // A 1-day half-life heavily discounts the 60-day-old row; 30-day barely.
    assert!(fresh[0].score < flat[0].score);
    cleanup(&path);
}

#[test]
fn future_occurred_at_clamped_in_scoring() {
    // occurred_at far in the future must not make the row score like fresh
    // forever: scoring clamps to now; since/until windows exclude it.
    let (e, path) = engine("future", orthogonal());
    let mut m = item("redis sharding plan");
    m.occurred_at = Some(MemoryItem::now() + 3650 * DAY);
    e.remember(&m).unwrap();
    let hits = e.recall(&q("redis sharding plan", 5)).unwrap();
    assert!(!hits.is_empty());
    // recency is clamped to 1.0 (age 0), never > 1: score must equal the
    // fresh-row score, i.e. final_score(fused, 1.0).
    let fresh_score = rank::final_score(hits[0].score / (0.9 + 0.1 * 1.0), 1.0);
    assert!((hits[0].score - fresh_score).abs() < 1e-9);
    // A --since window after the (future) event time excludes the row.
    let later = RecallQuery {
        text: "redis sharding plan".into(),
        k: 5,
        since: Some(MemoryItem::now() + 4000 * DAY),
        ..Default::default()
    };
    assert!(e.recall(&later).unwrap().is_empty());
    cleanup(&path);
}

#[test]
fn recency_knobs_compose() {
    // with_recency_off composes with with_half_life_days and both keep the
    // engine usable.
    let (e, path) = engine("compose", orthogonal());
    e.remember(&item("redis sharding plan")).unwrap();
    let hits = e
        .with_recency_off()
        .with_half_life_days(1.0)
        .recall(&q("redis sharding plan", 5))
        .unwrap();
    assert!(!hits.is_empty());
    assert!(hits
        .iter()
        .all(|h| !h.reasons.contains(&"recent".to_string())));
    cleanup(&path);
}
