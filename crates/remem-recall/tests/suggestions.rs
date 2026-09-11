//! recall-suggested edge candidates (issue #15 follow-up): the engine records
//! a (from_id, to_id, query, rank) candidate when a graph edge fuses a hit in;
//! `remem suggestions` lists them; accepting stays a manual `link --rel`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use remem_recall::RecallEngine;
use remem_store::Store;
use remem_types::{MemoryItem, MemoryKind, RecallQuery};

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
    let p = std::env::temp_dir().join(format!("remem-suggest-{tag}-{}-{n}.db", std::process::id()));
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

fn q(text: &str, k: usize) -> RecallQuery {
    RecallQuery {
        text: text.to_string(),
        k,
        ..Default::default()
    }
}

#[test]
fn suggestion_recorded_on_graph_fused_hit() {
    // Query hits only the hub; the satellite arrives purely via the graph edge.
    let (e, path) = engine("record", FakeEmbedder::new(&[("database tuning notes", 0)]));
    let hub_id = e.remember(&item("database tuning notes")).unwrap().0;
    let sat = item("completely different pastry");
    let sat_id = sat.id.clone();
    e.remember(&sat).unwrap();
    e.link(&hub_id, &sat_id, None).unwrap();
    let hits = e.recall(&q("database tuning notes", 5)).unwrap();
    let sat_hit = hits
        .iter()
        .find(|h| h.item.id == sat_id)
        .expect("linked memory recalled via graph");
    assert!(sat_hit.reasons.iter().any(|r| r.starts_with("graph#")));
    let rows = e.store().suggestions().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].from_id, hub_id);
    assert_eq!(rows[0].to_id, sat_id);
    assert_eq!(rows[0].query, "database tuning notes");
    // rank = the position of the hit in the returned list (1-based)
    assert!(rows[0].rank >= 1);
    // FTS/vector-only hits must not produce suggestions.
    cleanup(&path);
}

#[test]
fn fts_only_recall_records_no_suggestion() {
    let (e, path) = engine("nofuse", FakeEmbedder::new(&[("plain lexical query", 0)]));
    e.remember(&item("plain lexical query")).unwrap();
    e.remember(&item("another plain lexical entry")).unwrap();
    let hits = e.recall(&q("plain lexical", 5)).unwrap();
    assert!(!hits.is_empty());
    assert!(e.store().suggestions().unwrap().is_empty());
    cleanup(&path);
}

#[test]
fn suggestions_dedup_upserts_per_pair() {
    // Two recalls of the same pair: one row, refreshed query, count unchanged.
    let (e, path) = engine("dedup", FakeEmbedder::new(&[("database tuning notes", 0)]));
    let hub_id = e.remember(&item("database tuning notes")).unwrap().0;
    let sat = item("completely different pastry");
    let sat_id = sat.id.clone();
    e.remember(&sat).unwrap();
    e.link(&hub_id, &sat_id, None).unwrap();
    e.recall(&q("database tuning notes", 5)).unwrap();
    e.recall(&q("database tuning notes again", 5)).unwrap();
    let rows = e.store().suggestions().unwrap();
    assert_eq!(rows.len(), 1, "one row per memory pair");
    assert_eq!(rows[0].query, "database tuning notes again");
    assert_eq!(rows[0].count, 2);
    cleanup(&path);
}

#[test]
fn suggestions_pruned_after_30_days() {
    let (e, path) = engine("prune", FakeEmbedder::new(&[]));
    e.store()
        .record_suggestion("a", "b", "old query", 1)
        .unwrap();
    // Age the row past the 30-day window directly (no sleeping).
    e.store()
        .age_suggestions_for_test(MemoryItem::now() - 31 * 86_400)
        .unwrap();
    // Any new record triggers the prune pass.
    e.store()
        .record_suggestion("c", "d", "fresh query", 1)
        .unwrap();
    let rows = e.store().suggestions().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].query, "fresh query");
    cleanup(&path);
}

#[test]
fn suggestions_do_not_auto_link() {
    // Recording a suggestion must never write the edge itself (issue #15
    // regression guard): the pair stays suggestion-only until link --rel.
    let (e, path) = engine("noauto", FakeEmbedder::new(&[("database tuning notes", 0)]));
    let hub_id = e.remember(&item("database tuning notes")).unwrap().0;
    let sat = item("completely different pastry");
    let sat_id = sat.id.clone();
    e.remember(&sat).unwrap();
    let hits = e.recall(&q("database tuning notes", 5)).unwrap();
    assert!(hits.len() == 1 && hits[0].item.id == hub_id);
    // No graph edge, no suggestion (nothing fused through graph).
    assert!(e.store().suggestions().unwrap().is_empty());
    assert!(e.graph().unwrap().neighbors(&hub_id).unwrap().is_empty());
    cleanup(&path);
}
