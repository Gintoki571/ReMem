//! Corroboration gate (issue #11): remember() must decide merge-vs-split on
//! distinctive objects (tags + identifier spans + proper nouns). Exactly one
//! corroborating near neighbour merges into it (no new row); zero or 2+
//! corroborators (ambiguous) insert a new row. Value corrections — same
//! identity, changed value — never merge.

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
    let p = std::env::temp_dir().join(format!("remem-corr-{tag}-{}-{n}.db", std::process::id()));
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
fn exactly_one_corroborated_neighbour_merges() {
    // Same fact restated: identical distinctive object "8080", nothing new.
    let (e, path) = engine(
        "merge",
        FakeEmbedder::new(&[
            ("Parser runs on port 8080", 0),
            ("The parser service listens on 8080", 0),
        ]),
    );
    let first = item("Parser runs on port 8080", &["parser"]);
    let (id1, _) = e.remember(&first).unwrap();
    let restated = item("The parser service listens on 8080", &["parser"]);
    let (id2, _) = e.remember(&restated).unwrap();
    assert_eq!(id1, id2, "restatement must merge into the existing memory");
    assert_eq!(e.list().unwrap().len(), 1, "merge inserts no new row");
    cleanup(&path);
}

#[test]
fn value_correction_splits() {
    // Same identity ("parser"), changed value: 8080 -> 9090 must stay a
    // separate row, not silently overwrite the old fact.
    let (e, path) = engine(
        "correct",
        FakeEmbedder::new(&[
            ("Parser runs on port 8080", 0),
            ("Parser runs on port 9090", 0),
        ]),
    );
    let (old_id, _) = e
        .remember(&item("Parser runs on port 8080", &["parser"]))
        .unwrap();
    let (new_id, _) = e
        .remember(&item("Parser runs on port 9090", &["parser"]))
        .unwrap();
    assert_ne!(old_id, new_id, "value correction must split, not merge");
    let rows = e.list().unwrap();
    assert_eq!(rows.len(), 2, "both values stay on disk");
    assert!(rows.iter().any(|m| m.content.contains("8080")));
    assert!(rows.iter().any(|m| m.content.contains("9090")));
    cleanup(&path);
}

#[test]
fn ambiguous_two_corroborators_split() {
    // Two stored neighbours sit at the same vector as the new note and both
    // carry its distinctive objects: the gate cannot pick one, so the note
    // becomes its own row. Rows 2/3 are planted via the store directly so
    // they never passed through the gate themselves.
    let (e, path) = engine(
        "ambig",
        FakeEmbedder::new(&[
            ("Parser runs on port 8080", 0),
            ("Parser daemon binds 8080", 0),
            ("Parser service uses 8080", 0),
        ]),
    );
    e.remember(&item("Parser runs on port 8080", &["parser"]))
        .unwrap();
    for text in ["Parser daemon binds 8080", "Parser service uses 8080"] {
        let it = item(text, &["parser"]);
        let id = e.store().insert(&it).unwrap();
        let mut v = vec![0.0f32; 768];
        v[0] = 1.0;
        e.store().set_embedding(&id, &v).unwrap();
    }
    let (third, _) = e
        .remember(&item("Parser service uses 8080", &["parser"]))
        .unwrap();
    let rows = e.list().unwrap();
    assert_eq!(rows.len(), 3, "ambiguous corroboration inserts a new row");
    assert!(rows.iter().any(|m| m.id == third));
    cleanup(&path);
}
