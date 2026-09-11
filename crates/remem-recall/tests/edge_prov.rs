//! Edge provenance engine tests (issue #15): RED-first.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use remem_graph::EdgeProvenance;
use remem_recall::RecallEngine;
use remem_store::Store;
use remem_types::{MemoryItem, MemoryKind, RecallQuery};

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
    let p = std::env::temp_dir().join(format!("remem-prov-{tag}-{}-{n}.db", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
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

fn cleanup(path: &std::path::Path) {
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(PathBuf::from(format!("{}{}", path.display(), suffix)));
    }
}

fn tagged(content: &str, tags: &[&str]) -> MemoryItem {
    let mut m = MemoryItem::new(MemoryKind::Fact, content.to_string());
    m.tags = tags.iter().map(|t| t.to_string()).collect();
    m
}

#[test]
fn supersede_writes_correction_chain_edge() {
    let (e, path) = engine("sup", FakeEmbedder::new(&[]));
    let old = e
        .remember(&tagged("postgres listens on 5432", &["db"]))
        .unwrap()
        .0;
    let new_id = e
        .supersede(&old, &tagged("postgres listens on 5433", &["db"]))
        .unwrap();
    let edges = e.graph().unwrap().memory_edges().unwrap();
    assert_eq!(edges.len(), 1, "{edges:?}");
    assert_eq!(edges[0].from, new_id);
    assert_eq!(edges[0].to, old);
    assert_eq!(edges[0].rel, "SUPERSEDES");
    assert_eq!(edges[0].provenance, EdgeProvenance::CorrectionChain);
    cleanup(&path);
}

#[test]
fn cross_topic_supersedes_is_flagged_same_topic_is_clean() {
    // The report's mis-link probe: SUPERSEDES across unrelated topics must be
    // surfaced, not silently fused.
    let (e, path) = engine("sus", FakeEmbedder::new(&[]));
    let pg = e
        .remember(&tagged("postgres tuning notes", &["db"]))
        .unwrap()
        .0;
    let garden = e
        .remember(&tagged("rose pruning guide", &["garden"]))
        .unwrap()
        .0;
    let pg2 = e
        .remember(&tagged("postgres vacuum notes", &["db"]))
        .unwrap()
        .0;
    e.link(&pg, &garden, Some("SUPERSEDES")).unwrap();
    e.link(&pg, &pg2, Some("SUPERSEDES")).unwrap();
    let flags = e.suspect_supersedes().unwrap();
    assert_eq!(flags.len(), 1, "{flags:?}");
    assert!(
        flags[0].contains(&pg) && flags[0].contains(&garden),
        "{flags:?}"
    );
    assert!(flags[0].contains("no shared tags"), "{flags:?}");
    cleanup(&path);
}

#[test]
fn graph_hit_reasons_carry_provenance() {
    let (e, path) = engine(
        "reason",
        FakeEmbedder::new(&[("database tuning notes", 0), ("query", 0)]),
    );
    let hub = e
        .remember(&tagged("database tuning notes", &["db"]))
        .unwrap()
        .0;
    let sat = tagged("completely different pastry", &["db"]);
    let sat_id = sat.id.clone();
    e.remember(&sat).unwrap();
    e.link_with_provenance(&hub, &sat_id, Some("RELATES_TO"), "correction-chain")
        .unwrap();
    let hits = e
        .recall(&RecallQuery {
            text: "database tuning notes".into(),
            k: 5,
            ..Default::default()
        })
        .unwrap();
    let sat_hit = hits
        .iter()
        .find(|h| h.item.id == sat_id)
        .expect("linked hit");
    assert!(
        sat_hit.reasons.iter().any(|r| r.starts_with("graph#")),
        "{:?}",
        sat_hit.reasons
    );
    assert!(
        sat_hit
            .reasons
            .contains(&"prov:correction-chain".to_string()),
        "{:?}",
        sat_hit.reasons
    );
    cleanup(&path);
}
