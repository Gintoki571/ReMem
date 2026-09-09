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

/// The shipped defaults are docs/weight-spike.md row 1: k=30, fts 1.0 /
/// vector 0.5 / graph 1.0.
#[test]
fn default_weights_are_the_spike_winner() {
    assert_eq!(rank::DEFAULT_RRF_K, 30);
    let w = Weights::default();
    assert_eq!((w.fts, w.vector, w.graph), (1.0, 0.5, 1.0));
}

/// Importance is stored and reported (kind, `important` reason) but no longer
/// feeds the score: docs/weight-spike.md scored 35/37 with the term dropped, and
/// a writer-set 0.8 was outvoting dual `fts#1 + vector#1` agreement on Q7/Q12.
#[test]
fn importance_no_longer_moves_rank() {
    let (e, path) = engine("imp", FakeEmbedder::new(&[]));
    // Content must differ or the store's content-hash dedup collapses them.
    // The differing token is outside the query, so fusion ranks both the same
    // and list order (first-seen) decides.
    let mut low = item("same content text low");
    low.importance = 0.1;
    let mut high = item("same content text high");
    high.importance = 0.9;
    e.remember(&low).unwrap();
    e.remember(&high).unwrap();
    let hits = e.recall(&q("same content text", 5)).unwrap();
    assert_eq!(hits.len(), 2);
    // BM25 puts the low-importance row at fts#1; the fused lexical rank
    // decides, where the old 0.9 importance multiplier used to flip it.
    assert_eq!(hits[0].item.importance, 0.1, "importance must not reorder");
    assert!(hits[0].reasons.iter().any(|r| r == "fts#1"));
    // The signal is still reported to callers, just not scored.
    assert!(hits[1].reasons.contains(&"important".to_string()));
    cleanup(&path);
}

#[test]
fn recency_breaks_ties() {
    let (e, path) = engine("rec", FakeEmbedder::new(&[]));
    let now = MemoryItem::now();
    let mut old = item("stale keyword fact old");
    old.updated_at = now - 90 * DAY;
    old.created_at = old.updated_at;
    e.store().insert(&old).unwrap();
    let fresh = e.remember(&item("stale keyword fact new")).unwrap().0;
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
    let aid = e.remember(&a).unwrap().0;
    let bid = e.remember(&b).unwrap().0;
    let hits = e
        .recall(&RecallQuery {
            text: "deploy".into(),
            k: 5,
            kinds: Some(vec![MemoryKind::Decision]),
            ..Default::default()
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
            ..Default::default()
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
    let hub_id = e.remember(&item("database tuning notes")).unwrap().0;
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
    let id = e.remember(&m).unwrap().0;
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
fn k_zero_returns_empty_without_search() {
    let (e, path) = engine("k0", FakeEmbedder::new(&[]));
    e.remember(&item("shared keyword entry zero")).unwrap();
    let hits = e.recall(&q("shared keyword entry", 0)).unwrap();
    assert!(hits.is_empty(), "k=0 must return no results: {hits:?}");
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
    let id = e.remember(&item("temporary keyword note")).unwrap().0;
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

/// The two clocks: an event typed today about March must rank by March.
#[test]
fn recency_uses_occurred_at_over_created_at() {
    let (e, path) = engine("two-clocks", FakeEmbedder::new(&[]));
    let now = MemoryItem::now();
    let mut march = item("quarterly report keyword march");
    march.created_at = now; // typed today
    march.updated_at = now;
    march.occurred_at = Some(now - 90 * DAY); // happened in March
    let mid = e.store().insert(&march).unwrap();

    let recent = e
        .remember(&item("quarterly report keyword today"))
        .unwrap()
        .0;
    let hits = e.recall(&q("quarterly report keyword", 5)).unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].item.id, recent, "older event time must lose");
    let m = hits.iter().find(|h| h.item.id == mid).unwrap();
    assert!(!m.reasons.contains(&"recent".to_string()));
    // 30d half-life on the event clock, not the typing clock
    let r = rank::recency_score(march.occurred_at.unwrap(), 30.0, now);
    assert!((r - 0.125).abs() < 1e-6);
    cleanup(&path);
}

/// "What happened in March?" - date range over the event clock, falling back
/// to created_at for memories with no occurred_at.
#[test]
fn since_until_filter_on_event_time() {
    let (e, path) = engine("range", FakeEmbedder::new(&[]));
    let now = MemoryItem::now();
    let mut old_event = item("shared keyword march thing");
    old_event.created_at = now;
    old_event.updated_at = now;
    old_event.occurred_at = Some(now - 40 * DAY);
    let oid = e.store().insert(&old_event).unwrap();
    let mut no_event = item("shared keyword recent thing");
    no_event.created_at = now - 10 * DAY;
    no_event.updated_at = no_event.created_at;
    let nid = e.store().insert(&no_event).unwrap();
    let fresh = e.remember(&item("shared keyword fresh thing")).unwrap().0;

    let window = |since: i64, until: i64| {
        e.recall(&RecallQuery {
            text: "shared keyword".into(),
            k: 10,
            since: Some(since),
            until: Some(until),
            ..Default::default()
        })
        .unwrap()
    };
    // The 30-day band: the March event is out (its event clock is older),
    // the no-occurred_at row is in via its created_at, and so is fresh.
    let ids: Vec<_> = window(now - 30 * DAY, now + DAY)
        .into_iter()
        .map(|h| h.item.id)
        .collect();
    assert!(!ids.contains(&oid), "march event excluded: {ids:?}");
    assert!(ids.contains(&nid) && ids.contains(&fresh), "got {ids:?}");
    // A band covering the older event pulls it in and drops the rest.
    let ids: Vec<_> = window(now - 50 * DAY, now - 30 * DAY)
        .into_iter()
        .map(|h| h.item.id)
        .collect();
    assert_eq!(ids, vec![oid]);
    // Unbounded query is unchanged.
    assert_eq!(window(now - 365 * DAY, now + DAY).len(), 3);
    cleanup(&path);
}

/// Token-budget packing: a long top hit is skipped so shorter, still-relevant
/// hits fit, and the budget never silently blows the caller's context.
#[test]
fn max_chars_skips_long_top_hit_for_fitting_hits() {
    let (e, path) = engine("budget-skip", FakeEmbedder::new(&[]));
    let long = e
        .remember(&item(&format!(
            "shared keyword entry long {}",
            "z".repeat(200)
        )))
        .unwrap()
        .0;
    let a = e.remember(&item("shared keyword entry a")).unwrap().0;
    let b = e.remember(&item("shared keyword entry b")).unwrap().0;
    let hits = e
        .recall(&RecallQuery {
            text: "shared keyword entry".into(),
            k: 5,
            max_chars: Some(80),
            ..Default::default()
        })
        .unwrap();
    let ids: Vec<_> = hits.iter().map(|h| h.item.id.clone()).collect();
    assert!(!ids.contains(&long), "over-budget top hit leaked: {ids:?}");
    assert!(
        ids.contains(&a) && ids.contains(&b),
        "fitting hits lost: {ids:?}"
    );
    let used: usize = hits.iter().map(|h| h.item.content.len()).sum();
    assert!(used <= 80, "budget exceeded: {used}");
    cleanup(&path);
}

/// With nothing under budget, the top hit still comes back whole.
#[test]
fn max_chars_always_keeps_top_hit() {
    let (e, path) = engine("budget-top", FakeEmbedder::new(&[]));
    let top = e
        .remember(&item(&format!("solo keyword fact {}", "y".repeat(200))))
        .unwrap()
        .0;
    let hits = e
        .recall(&RecallQuery {
            text: "solo keyword fact".into(),
            k: 5,
            max_chars: Some(1),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].item.id, top);
    cleanup(&path);
}

/// No budget means the current top-k behaviour is unchanged.
#[test]
fn no_max_chars_keeps_everything() {
    let (e, path) = engine("budget-off", FakeEmbedder::new(&[]));
    e.remember(&item(&format!("plain keyword note {}", "x".repeat(200))))
        .unwrap();
    let hits = e
        .recall(&RecallQuery {
            text: "plain keyword note".into(),
            k: 5,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(hits.len(), 1);
    cleanup(&path);
}

/// Open a second engine on an existing db file (the store/graph are file-backed,
/// so a floor test can read what the first engine wrote).
fn engine_on(path: &std::path::Path, embed: FakeEmbedder) -> RecallEngine {
    let store = Store::open(path.to_str().unwrap()).unwrap();
    let graph = remem_graph::Graph::open(path).unwrap();
    RecallEngine::new(store, Box::new(embed)).with_graph(graph)
}

/// Score floor: a floor above every score returns nothing instead of junk,
/// a floor below the real hit keeps it, and the default 0.0 is off.
#[test]
fn min_score_floor_drops_junk_and_keeps_real_hits() {
    let (e, path) = engine("floor", FakeEmbedder::new(&[]));
    e.remember(&item("staging postgres listens on port 5432"))
        .unwrap();
    e.remember(&item("banana split recipe needs bananas"))
        .unwrap();
    drop(e);

    let unfloored = engine_on(&path, FakeEmbedder::new(&[]))
        .recall(&q("staging postgres port", 5))
        .unwrap();
    assert!(!unfloored.is_empty(), "default floor must change nothing");
    let top = unfloored[0].score;

    let kept = engine_on(&path, FakeEmbedder::new(&[]))
        .with_min_score(top * 0.5)
        .recall(&q("staging postgres port", 5))
        .unwrap();
    assert!(
        kept.iter().any(|h| h.item.content.contains("port 5432")),
        "kept: {kept:?}"
    );

    // Above the best score: even the top hit is dropped, so recall is empty.
    let empty = engine_on(&path, FakeEmbedder::new(&[]))
        .with_min_score(top * 2.0)
        .recall(&q("staging postgres port", 5))
        .unwrap();
    assert!(empty.is_empty(), "floor above top hit must bail: {empty:?}");

    cleanup(&path);
}

/// Build a Q27-shaped store: the query's only lexical anchor for the target is
/// its tag. Returns the recalled contents in rank order.
fn tag_q27_store(tag: &str) -> (RecallEngine, PathBuf) {
    let (e, path) = engine(
        "tagboost",
        FakeEmbedder::new(&[
            ("what did we decide about spending", 0),
            ("infra agent retried the flaky sync job", 0),
            ("the load test reached nine thousand requests", 0),
            ("quarterly numbers are reviewed by finance", 1),
        ]),
    );
    let mut target = MemoryItem::new(
        MemoryKind::Decision,
        "quarterly numbers are reviewed by finance".to_string(),
    );
    target.tags = vec![tag.to_string()];
    e.remember(&target).unwrap();
    // The other three carry no tag and share the query's vector exactly, so
    // they can only be beaten by a signal outside FTS/vector.
    for c in [
        "infra agent retried the flaky sync job",
        "the load test reached nine thousand requests",
    ] {
        let mut d = MemoryItem::new(MemoryKind::Note, c.to_string());
        d.tags = vec!["ops".to_string()];
        e.remember(&d).unwrap();
    }
    (e, path)
}

/// Q27 from docs/eval.md: "decide" appears in no content, only in the tag
/// `decision`, so the tag channel is the only signal that can move the target.
/// Per docs/tag-supervision.md the gated 2.0x pays only on FTS-absent hits;
/// every hit here is FTS-absent and single-list, so the target doubles past
/// all vector-only rivals to the top. (On the real Q27 dual fts+vector rivals
/// cap the lift at rank 3-4.)
/// no_tag_overlap_... is the control: same store, tag renamed, no move.
#[test]
fn tag_word_in_query_lifts_tagged_target_one_place() {
    let (e, path) = tag_q27_store("decision");
    let hits = e
        .recall(&q("what did we decide about spending", 5))
        .unwrap();
    let order: Vec<&str> = hits.iter().map(|h| h.item.content.as_str()).collect();
    let rank = order
        .iter()
        .position(|c| *c == "quarterly numbers are reviewed by finance")
        .expect("target present");
    assert_eq!(
        rank, 0,
        "gated tag boost must lift the FTS-absent target to the top: {order:?}"
    );
    assert!(hits[rank].reasons.iter().any(|r| r == "tag"));
    drop(e);
    cleanup(&path);
}

#[test]
fn no_tag_overlap_leaves_ranking_unchanged() {
    let (e, path) = tag_q27_store("finance");
    let hits = e
        .recall(&q("what did we decide about spending", 5))
        .unwrap();
    let order: Vec<&str> = hits.iter().map(|h| h.item.content.as_str()).collect();
    assert_ne!(
        order[0], "quarterly numbers are reviewed by finance",
        "unrelated tag must not lift the target: {order:?}"
    );
    assert!(!hits.iter().any(|h| h.reasons.iter().any(|r| r == "tag")));
    cleanup(&path);
}

/// Embedder with explicit vectors per text (unknown text -> e767, far from
/// dims 0..N), so a test can place memories at an exact L2 distance.
struct VecEmbedder {
    map: HashMap<String, Vec<f32>>,
}

impl VecEmbedder {
    fn new(pairs: &[(&str, Vec<f32>)]) -> Self {
        Self {
            map: pairs
                .iter()
                .map(|(t, v)| (t.to_string(), v.clone()))
                .collect(),
        }
    }
}

fn dim(i: usize, w: f32) -> Vec<f32> {
    let mut v = vec![0.0f32; 768];
    v[i] = w;
    v
}

/// cos(a, b) = 0.95 -> L2 = 0.3162; the e1-only vector below is at 1.0.
fn paraphrase_of(i: usize) -> Vec<f32> {
    let mut v = dim(i, 0.95);
    v[i + 1] = (1.0 - 0.95f32 * 0.95).sqrt();
    v
}

impl remem_recall::Embed for VecEmbedder {
    fn embed(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        Ok(texts
            .iter()
            .map(|t| self.map.get(*t).cloned().unwrap_or_else(|| dim(767, 1.0)))
            .collect())
    }
}

fn vec_engine(tag: &str, pairs: &[(&str, Vec<f32>)]) -> (RecallEngine, PathBuf) {
    let path = temp_db(tag);
    let store = Store::open(path.to_str().unwrap()).unwrap();
    let graph = remem_graph::Graph::open(&path).unwrap();
    (
        RecallEngine::new(store, Box::new(VecEmbedder::new(pairs))).with_graph(graph),
        path,
    )
}

/// The near-duplicate write probe: a paraphrase at cos 0.95 (L2 0.316, inside
/// SIMILAR_MAX_DISTANCE) is reported with its id and distance; a distinct
/// memory at L2 1.0 is not. The probe runs before insert, so the new memory
/// itself never appears, and the first write into an empty store is silent.
#[test]
fn remember_reports_near_duplicate_paraphrase_only() {
    let a = "we ship with cargo because it is fast";
    let b = "we ship with cargo because it is quick"; // paraphrase: dim-0 mix
    let c = "unrelated pastry content"; // orthogonal
    let (e, path) = vec_engine(
        "near-dup",
        &[(a, dim(0, 1.0)), (b, paraphrase_of(0)), (c, dim(1, 1.0))],
    );

    let (ida, sim0) = e.remember(&item(a)).unwrap();
    assert!(sim0.is_empty(), "empty store must report nothing: {sim0:?}");

    let (idb, sim) = e.remember(&item(b)).unwrap();
    assert_ne!(idb, ida);
    assert_eq!(sim.len(), 1, "only the paraphrase is near: {sim:?}");
    assert_eq!(sim[0].0, ida);
    assert!((sim[0].1 - 0.3162).abs() < 1e-3, "distance {sim:?}");

    let (_, sim) = e.remember(&item(c)).unwrap();
    assert!(sim.is_empty(), "distinct memory must stay silent: {sim:?}");
    cleanup(&path);
}

/// The calibrated boundary: cos 0.95 (L2 0.316) fires, cos 0.80 (L2 0.632)
/// does not. The gap is fixture-measured (see SIMILAR_MAX_DISTANCE), so a
/// threshold change that swallows either side shows up here.
#[test]
fn near_dup_threshold_sits_between_paraphrase_and_distinct() {
    let a = "anchor memory content one";
    let close = "cos 0.95 rewrite of the anchor";
    let far = "cos 0.80 rewrite of the anchor";
    let mut v = dim(0, 0.8);
    v[1] = (1.0 - 0.64f32).sqrt();
    let (e, path) = vec_engine(
        "near-dup-edge",
        &[(a, dim(0, 1.0)), (close, paraphrase_of(0)), (far, v)],
    );
    let ida = e.remember(&item(a)).unwrap().0;

    // cos 0.80 against the anchor only: silent.
    let (_, sim) = e.remember(&item(far)).unwrap();
    assert!(sim.is_empty(), "cos 0.80 must stay silent: {sim:?}");
    // cos 0.95 now sees the anchor (0.316) and the cos-0.80 row (0.324 from
    // close, so it fires too); what matters is that the anchor is reported.
    let (_, sim) = e.remember(&item(close)).unwrap();
    assert!(
        sim.iter().any(|(id, _)| *id == ida),
        "cos 0.95 must fire: {sim:?}"
    );
    cleanup(&path);
}

/// The probe is capped at 3 neighbours, so a writer gets the signal without a
/// dump when a whole cluster of near-identical re-saves already exists.
#[test]
fn near_dup_caps_at_three_neighbours() {
    let mut pairs = vec![("probe anchor", dim(0, 1.0))];
    for i in 0..5 {
        pairs.push((
            Box::leak(format!("probe rewrite {i}").into_boxed_str()),
            paraphrase_of(0),
        ));
    }
    let (e, path) = vec_engine("near-dup-cap", &pairs);
    e.remember(&item("probe anchor")).unwrap();
    for i in 0..5 {
        let (_, sim) = e.remember(&item(&format!("probe rewrite {i}"))).unwrap();
        assert!(sim.len() <= 3, "probe returned {}: {sim:?}", sim.len());
        assert!(!sim.is_empty(), "rewrites are near-dups: {sim:?}");
    }
    cleanup(&path);
}

/// An exact re-save dedups on content_hash and must not double-report: the
/// call returns the original id with an empty similar list (the probe would
/// otherwise find the stored copy at distance 0).
#[test]
fn exact_duplicate_dedups_and_reports_no_similar() {
    let (e, path) = vec_engine("exact-dup", &[("same content twice", dim(0, 1.0))]);
    let (ida, _) = e.remember(&item("same content twice")).unwrap();
    let mut again = item("same content twice");
    again.id = "second-uuid".to_string();
    let (id2, sim) = e.remember(&again).unwrap();
    assert_eq!(id2, ida, "exact dup must return the stored id");
    assert!(
        sim.is_empty(),
        "exact dup must not also report similar: {sim:?}"
    );
    assert_eq!(e.stats().unwrap()["memories"], 1);
    cleanup(&path);
}

#[test]
fn content_free_queries_abstain_and_content_searches() {
    let (e, path) = engine("content-free", FakeEmbedder::new(&[]));
    e.remember(&item("redis sharding routes keys by hash slot"))
        .unwrap();
    assert!(e.recall(&q("what is the thing", 5)).unwrap().is_empty());
    assert!(e.recall(&q("the", 5)).unwrap().is_empty());
    let hits = e.recall(&q("redis sharding", 5)).unwrap();
    assert!(!hits.is_empty());
    assert!(hits[0].item.content.contains("redis sharding"));
    cleanup(&path);
}
