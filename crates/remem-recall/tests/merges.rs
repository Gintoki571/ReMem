//! Traceable corroboration merges (issue #14): remember() records the
//! absorbed phrasing in a `merges` table; recall can surface it; `remem
//! trace` lists absorbed rows under the survivor.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use remem_recall::{Embed, RecallEngine};
use remem_store::Store;
use remem_types::{MemoryItem, MemoryKind};

struct FakeEmbedder {
    map: std::collections::HashMap<String, usize>,
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
    let p = std::env::temp_dir().join(format!("remem-merge-{tag}-{}-{n}.db", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

fn item(content: &str, tags: &[&str]) -> MemoryItem {
    let mut it = MemoryItem::new(MemoryKind::Fact, content.to_string());
    it.tags = tags.iter().map(|s| s.to_string()).collect();
    it
}

fn engine(tag: &str, embed: FakeEmbedder) -> (RecallEngine, PathBuf) {
    let path = temp_db(tag);
    let store = Store::open(path.to_str().unwrap()).unwrap();
    (RecallEngine::new(store, Box::new(embed)), path)
}

fn cleanup(path: &std::path::Path) {
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(PathBuf::from(format!("{}{}", path.display(), suffix)));
    }
}

#[test]
fn merge_records_absorbed_content() {
    let (e, path) = engine(
        "record",
        FakeEmbedder::new(&[
            ("Parser runs on port 8080", 0),
            ("The parser service listens on 8080", 0),
        ]),
    );
    let first = item("Parser runs on port 8080", &["parser"]);
    let (id1, _) = e.remember(&first).unwrap();
    let restated = item("The parser service listens on 8080", &["parser"]);
    let (id2, _) = e.remember(&restated).unwrap();
    assert_eq!(id1, id2, "restatement merges into existing row");

    // The absorbed phrasing is recorded against the survivor.
    let merged = e.store().merges_for(&id1).unwrap();
    assert_eq!(merged.len(), 1, "one merge row: {merged:?}");
    assert_eq!(merged[0].survivor_id, id1);
    assert_eq!(
        merged[0].absorbed_content,
        "The parser service listens on 8080"
    );
    assert!(merged[0].absorbed_at > 0, "absorbed_at must be set");
    cleanup(&path);
}

#[test]
fn split_writes_no_merge_row() {
    let (e, path) = engine(
        "split",
        FakeEmbedder::new(&[
            ("Alpha fact one", 0),
            ("Beta fact two", 1),
            ("Gamma fact three", 2),
        ]),
    );
    e.remember(&item("Alpha fact one", &["a"])).unwrap();
    e.remember(&item("Beta fact two", &["b"])).unwrap();
    assert!(
        e.store().merges_for("nonexistent").unwrap().is_empty(),
        "no merges recorded"
    );
    cleanup(&path);
}

#[test]
fn recall_finds_absorbed_phrasing() {
    let (e, path) = engine(
        "recall-absorbed",
        FakeEmbedder::new(&[
            ("Parser runs on port 8080", 0),
            ("The parser service listens on 8080", 0),
        ]),
    );
    e.remember(&item("Parser runs on port 8080", &["parser"]))
        .unwrap();
    let restated = item("The parser service listens on 8080", &["parser"]);
    e.remember(&restated).unwrap();

    // The absorbed phrasing ("listens on") is searchable.
    let hits = e
        .recall(&remem_types::RecallQuery {
            text: "listens on 8080".into(),
            k: 5,
            ..Default::default()
        })
        .unwrap();
    assert!(
        hits.iter()
            .any(|h| h.item.content == "Parser runs on port 8080"),
        "absorbed phrasing must be recallable, got: {:?}",
        hits.iter().map(|h| &h.item.content).collect::<Vec<_>>()
    );
    cleanup(&path);
}

#[test]
fn trace_lists_merges_under_survivor() {
    let (e, path) = engine(
        "trace",
        FakeEmbedder::new(&[
            ("Parser runs on port 8080", 0),
            ("The parser service listens on 8080", 0),
        ]),
    );
    let (id1, _) = e
        .remember(&item("Parser runs on port 8080", &["parser"]))
        .unwrap();
    e.remember(&item("The parser service listens on 8080", &["parser"]))
        .unwrap();

    let out = e.store().merge_trace(&id1).unwrap();
    assert_eq!(out.len(), 1, "one absorbed row under survivor: {out:?}");
    assert_eq!(out[0], "The parser service listens on 8080");
    cleanup(&path);
}
