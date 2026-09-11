//! Edge provenance (issue #15): RED-first. These fail until implemented.

use remem_graph::{EdgeProvenance, Graph};
use remem_types::MemoryKind;

fn graph() -> Graph {
    Graph::open_in_memory().expect("open in-memory graph")
}

fn two() -> Graph {
    let g = graph();
    g.upsert_memory("m1", MemoryKind::Fact).unwrap();
    g.upsert_memory("m2", MemoryKind::Fact).unwrap();
    g
}

#[test]
fn link_defaults_to_manual_provenance() {
    let g = two();
    g.link("m1", "m2", "RELATES_TO").unwrap();
    let d = g.neighbors_detail("m1").unwrap();
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].provenance, EdgeProvenance::Manual);
}

#[test]
fn link_with_provenance_roundtrips() {
    let g = two();
    g.link_with_provenance("m1", "m2", "SUPERSEDES", EdgeProvenance::CorrectionChain)
        .unwrap();
    let d = g.neighbors_detail("m1").unwrap();
    assert_eq!(d[0].provenance, EdgeProvenance::CorrectionChain);
    // MERGE keeps the provenance on re-link with the same value
    g.link_with_provenance("m1", "m2", "SUPERSEDES", EdgeProvenance::CorrectionChain)
        .unwrap();
    assert_eq!(
        g.neighbors_detail("m1").unwrap()[0].provenance,
        EdgeProvenance::CorrectionChain
    );
}

#[test]
fn unknown_provenance_rejected() {
    let g = two();
    let err = g
        .link_with_provenance_str("m1", "m2", "RELATES_TO", "llm-guess")
        .unwrap_err();
    assert!(err.to_string().contains("unknown provenance"), "{err}");
}

#[test]
fn validate_lists_non_manual_edges_only() {
    let g = two();
    g.upsert_memory("m3", MemoryKind::Fact).unwrap();
    g.link("m1", "m2", "RELATES_TO").unwrap();
    g.link_with_provenance("m1", "m3", "SUPERSEDES", EdgeProvenance::CorrectionChain)
        .unwrap();
    let v = g.validate();
    assert!(
        v.iter()
            .any(|l| l.contains("m1") && l.contains("m3") && l.contains("correction-chain")),
        "validate must surface the correction-chain edge, got {v:?}"
    );
    assert!(
        !v.iter().any(|l| l.contains("m2") && !l.contains("m3")),
        "manual edge must stay silent, got {v:?}"
    );
}
