//! Write-time auto-link: after insert, remember() runs a vec0 kNN (top-3,
//! cosine >= 0.72) against existing memories. Exactly one passing candidate
//! writes an `auto-link` edge; 2-3 park ALL in suggested_edges. Zero LLM.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use remem_recall::RecallEngine;
use remem_store::Store;
use remem_types::{MemoryItem, MemoryKind};

/// Graded fake embedder: maps each text to a 768-dim unit vector
/// [cos(angle), sin(angle), 0...], so pairwise cosine = cos(angle difference).
struct GradedEmbedder {
    angles: Vec<(&'static str, f64)>,
}

impl GradedEmbedder {
    fn new(angles: &[(&'static str, f64)]) -> Self {
        Self {
            angles: angles.to_vec(),
        }
    }
}

impl remem_recall::Embed for GradedEmbedder {
    fn embed(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        Ok(texts
            .iter()
            .map(|t| {
                let a = self
                    .angles
                    .iter()
                    .find(|(s, _)| s == t)
                    .map(|(_, a)| *a)
                    .unwrap_or(std::f64::consts::PI);
                let mut v = vec![0.0f32; 768];
                v[0] = a.cos() as f32;
                v[1] = a.sin() as f32;
                v
            })
            .collect())
    }
}

fn temp_db(tag: &str) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "remem-automlink-{tag}-{}-{n}.db",
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

fn item(content: &str) -> MemoryItem {
    MemoryItem::new(MemoryKind::Fact, content.to_string())
}

fn engine(tag: &str, embed: GradedEmbedder) -> (RecallEngine, PathBuf) {
    let path = temp_db(tag);
    let store = Store::open(path.to_str().unwrap()).unwrap();
    let graph = remem_graph::Graph::open(&path).unwrap();
    (
        RecallEngine::new(store, Box::new(embed)).with_graph(graph),
        path,
    )
}

/// Auto-link writes an edge (provenance auto-link) when exactly ONE candidate
/// clears cosine 0.72, and nothing when none does.
#[test]
fn auto_link_unique_candidate_writes_edge_unrelated_stays_alone() {
    // Chain: r1-r2 cos 0.906, r2-r3 cos 0.906, r1-r3 cos 0.643 (< 0.72).
    // Unrelated sits at 120 deg: <= cos 70 deg = 0.342 to everything.
    let (e, path) = engine(
        "unique",
        GradedEmbedder::new(&[
            ("note one", 0.0),
            ("note two", 25.0_f64.to_radians()),
            ("note three", 50.0_f64.to_radians()),
            ("banana unrelated", 120.0_f64.to_radians()),
        ]),
    );
    let id1 = e.remember(&item("note one")).unwrap().0;
    let id2 = e.remember(&item("note two")).unwrap().0;
    let id3 = e.remember(&item("note three")).unwrap().0;
    let idu = e.remember(&item("banana unrelated")).unwrap().0;

    let g = e.graph().unwrap();
    let edges = g.memory_edges().unwrap();
    // Exactly the two chain edges, each auto-link provenance.
    let has = |from: &str, to: &str| {
        edges.iter().any(|ed| {
            ed.from == from && ed.to == to && ed.provenance == remem_graph::EdgeProvenance::AutoLink
        })
    };
    assert!(
        has(&id2, &id1),
        "r2 -> r1 auto-link missing, edges: {edges:?}"
    );
    assert!(
        has(&id3, &id2),
        "r3 -> r2 auto-link missing, edges: {edges:?}"
    );
    assert_eq!(edges.len(), 2, "unexpected extra edges: {edges:?}");
    assert!(
        edges.iter().all(|ed| ed.from != idu && ed.to != idu),
        "unrelated memory must get no edges: {edges:?}"
    );
    cleanup(&path);
}

/// 2-3 passing candidates park ALL in suggested_edges; no auto-link edge.
#[test]
fn auto_link_multi_match_parks_in_suggested_edges() {
    // r2 and r3 are identical (cos 1.0) and both close to r1 (0.866).
    let (e, path) = engine(
        "multi",
        GradedEmbedder::new(&[
            ("hub note", 0.0),
            ("twin a", 30.0_f64.to_radians()),
            ("twin b", 30.0_f64.to_radians()),
        ]),
    );
    let id1 = e.remember(&item("hub note")).unwrap().0;
    let id2 = e.remember(&item("twin a")).unwrap().0;
    let id3 = e.remember(&item("twin b")).unwrap().0;

    // r3 saw two candidates (r1, r2) -> parked, no edge from r3.
    let store = e.store();
    let sug = store.suggestions().unwrap();
    // record_suggestion canonicalizes each pair to (min, max).
    let pair = |a: &str, b: &str| (a.min(b).to_string(), a.max(b).to_string());
    let pairs: Vec<(String, String)> = sug
        .iter()
        .map(|s| (s.from_id.clone(), s.to_id.clone()))
        .collect();
    assert!(
        pairs.contains(&pair(&id3, &id1)),
        "r3-r1 not parked: {pairs:?}"
    );
    assert!(
        pairs.contains(&pair(&id3, &id2)),
        "r3-r2 not parked: {pairs:?}"
    );

    let g = e.graph().unwrap();
    let edges = g.memory_edges().unwrap();
    // Only the unique-match edge r2 -> r1 exists; nothing from r3.
    assert_eq!(edges.len(), 1, "expected only r2->r1 auto-link: {edges:?}");
    assert_eq!(edges[0].from, id2);
    assert_eq!(edges[0].to, id1);
    assert_eq!(edges[0].provenance, remem_graph::EdgeProvenance::AutoLink);
    assert!(
        edges.iter().all(|ed| ed.from != id3 && ed.to != id3),
        "multi-match must not auto-link: {edges:?}"
    );
    cleanup(&path);
}

/// Provenance round-trip: "auto-link" parses and prints, and the write APIs
/// accept it.
#[test]
fn auto_link_provenance_parses_and_links() {
    assert_eq!(
        remem_graph::EdgeProvenance::parse("auto-link"),
        Some(remem_graph::EdgeProvenance::AutoLink)
    );
    let (e, path) = engine(
        "parse",
        GradedEmbedder::new(&[("alpha", 0.0), ("beta", 90.0_f64.to_radians())]),
    );
    let id1 = e.remember(&item("alpha")).unwrap().0;
    let id2 = e.remember(&item("beta")).unwrap().0;
    e.link_with_provenance(&id1, &id2, None, "auto-link")
        .unwrap();
    let edges = e.graph().unwrap().memory_edges().unwrap();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].provenance, remem_graph::EdgeProvenance::AutoLink);
    cleanup(&path);
}
