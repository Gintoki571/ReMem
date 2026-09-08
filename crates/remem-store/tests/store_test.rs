use remem_store::Store;
use remem_types::{MemoryItem, MemoryKind};

fn item(content: &str) -> MemoryItem {
    let mut m = MemoryItem::new(MemoryKind::Note, content.to_string());
    m.tags = vec!["test".into()];
    m.agent_id = "agent-1".into();
    m.session_id = "sess-1".into();
    m.importance = 0.8;
    m
}

#[test]
fn insert_get_roundtrip() {
    let store = Store::open(":memory:").unwrap();
    let m = item("rust borrows are checked at compile time");
    let id = store.insert(&m).unwrap();
    let got = store.get(&id).unwrap();
    assert_eq!(got.id, id);
    assert_eq!(got.content, m.content);
    assert_eq!(got.kind, m.kind);
    assert_eq!(got.tags, m.tags);
    assert_eq!(got.agent_id, "agent-1");
    assert_eq!(got.importance, 0.8);
    assert!(store.get("no-such-id").is_none());
}

#[test]
fn fts_finds_keywords_and_ranks() {
    let store = Store::open(":memory:").unwrap();
    let a = item("the parser walks the token stream");
    let b = item("database schema migration steps");
    let ia = store.insert(&a).unwrap();
    let _ib = store.insert(&b).unwrap();
    let hits = store.fts_search("parser tokens", 10).unwrap();
    assert!(!hits.is_empty());
    assert_eq!(hits[0].0.id, ia);
    assert!(
        hits[0].1 <= 0.0,
        "bm25 rank should be negative-ish, got {}",
        hits[0].1
    );
    assert!(store.fts_search("zzz-no-match", 10).unwrap().is_empty());
}

#[test]
fn fts_survives_filler_words_in_nl_query() {
    let store = Store::open(":memory:").unwrap();
    let a = item("the parser walks the token stream");
    let ia = store.insert(&a).unwrap();
    // Every content token ANDed would match nothing; OR after stopword drop hits.
    let hits = store.fts_search("what is the parser", 10).unwrap();
    assert_eq!(hits[0].0.id, ia);
    let hits = store
        .fts_search("what is the parser token stream about", 10)
        .unwrap();
    assert_eq!(hits[0].0.id, ia);
}

#[test]
fn fts_stays_in_sync_on_update_and_delete() {
    let store = Store::open(":memory:").unwrap();
    let mut m = item("original text about butterflies");
    let id = store.insert(&m).unwrap();
    assert_eq!(store.fts_search("butterflies", 5).unwrap().len(), 1);
    m.id = id.clone();
    m.content = "revised text about dragonflies".into();
    store.update(&m).unwrap();
    assert!(store.fts_search("butterflies", 5).unwrap().is_empty());
    assert_eq!(store.fts_search("dragonflies", 5).unwrap()[0].0.id, id);
    store.delete(&id).unwrap();
    assert!(store.fts_search("dragonflies", 5).unwrap().is_empty());
    assert!(store.get(&id).is_none(), "soft-deleted rows read as gone");
}

#[test]
fn set_embedding_and_knn_nearest_first() {
    let store = Store::open(":memory:").unwrap();
    // hand-made vectors: id-nearest shares direction with query
    let q = one_hot(0);
    let near = store.insert(&item("memory near")).unwrap();
    let far = store.insert(&item("memory far")).unwrap();
    let off = store.insert(&item("memory off")).unwrap();
    store.set_embedding(&near, &axis_vec(0, 0.9)).unwrap();
    store.set_embedding(&far, &axis_vec(3, 0.9)).unwrap();
    store.set_embedding(&off, &axis_vec(6, 1.0)).unwrap();
    let hits = store.knn(&q, 2).unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].0, near);
    assert_eq!(hits[1].0, far);
    assert!(hits[0].1 < hits[1].1);
    // third match (off) must be excluded by k=2
    let all = store.knn(&q, 3).unwrap();
    assert_eq!(all.len(), 3);
    assert_eq!(all[2].0, off);
}

#[test]
fn knn_skips_memories_without_embedding_and_deleted() {
    let store = Store::open(":memory:").unwrap();
    let a = store.insert(&item("alpha")).unwrap();
    let b = store.insert(&item("beta")).unwrap();
    store.set_embedding(&a, &axis_vec(1, 1.0)).unwrap();
    // b has no embedding; soft-delete a separate memory with embedding
    let c = store.insert(&item("gamma")).unwrap();
    store.set_embedding(&c, &axis_vec(2, 1.0)).unwrap();
    store.delete(&c).unwrap();
    let hits = store.knn(&axis_vec(1, 1.0), 10).unwrap();
    assert_eq!(hits, vec![(a.clone(), 0.0f32)]);
    let _ = b;
}

#[test]
fn wrong_dimension_embedding_is_rejected() {
    let store = Store::open(":memory:").unwrap();
    let id = store.insert(&item("short")).unwrap();
    assert!(store.set_embedding(&id, &[0.1, 0.2]).is_err());
    assert!(store.knn(&[0.1, 0.2], 3).is_err());
}

#[test]
fn file_store_reopens_with_data() {
    let dir = std::env::temp_dir().join(format!("remem-store-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("test.db");
    let _ = std::fs::remove_file(&path);
    let id = {
        let store = Store::open(path.to_str().unwrap()).unwrap();
        store.insert(&item("persist me")).unwrap()
    };
    let store = Store::open(path.to_str().unwrap()).unwrap();
    assert_eq!(store.get(&id).unwrap().content, "persist me");
    assert_eq!(store.fts_search("persist", 5).unwrap().len(), 1);
    std::fs::remove_file(&path).ok();
}

#[test]
fn list_respects_deleted_flag() {
    let store = Store::open(":memory:").unwrap();
    let a = store.insert(&item("keep")).unwrap();
    let b = store.insert(&item("drop")).unwrap();
    store.delete(&b).unwrap();
    let live = store.list(false).unwrap();
    assert!(live.iter().any(|m| m.id == a));
    assert!(live.iter().all(|m| m.id != b));
    assert_eq!(store.list(true).unwrap().len(), 2);
}

#[test]
fn duplicate_insert_returns_same_id_and_no_new_row() {
    let store = Store::open(":memory:").unwrap();
    let id1 = store.insert(&item("the same lesson twice")).unwrap();
    let id2 = store.insert(&item("the same lesson twice")).unwrap();
    assert_eq!(id1, id2);
    assert_eq!(store.list(false).unwrap().len(), 1);
}

#[test]
fn near_duplicate_case_and_whitespace_dedups() {
    let store = Store::open(":memory:").unwrap();
    let id1 = store.insert(&item("  Spaced   OUT Lesson ")).unwrap();
    let id2 = store.insert(&item("spaced out lesson")).unwrap();
    assert_eq!(id1, id2);
    assert_eq!(store.list(false).unwrap().len(), 1);
    assert!(store.find_by_hash("nope").is_none());
    let hash = remem_store::content_hash(&MemoryKind::Note, "SPACED   out LESSON");
    assert_eq!(store.find_by_hash(&hash).unwrap(), id1);
}

#[test]
fn same_text_different_kind_does_not_dedup() {
    let store = Store::open(":memory:").unwrap();
    let mut fact = item("shared text here");
    fact.kind = MemoryKind::Fact;
    let mut note = item("shared text here");
    note.kind = MemoryKind::Note;
    let id1 = store.insert(&fact).unwrap();
    let id2 = store.insert(&note).unwrap();
    assert_ne!(id1, id2);
    assert_eq!(store.list(false).unwrap().len(), 2);
}

#[test]
fn purge_removes_row_fts_and_vec() {
    let store = Store::open(":memory:").unwrap();
    let id = store.insert(&item("secret tokens live here")).unwrap();
    store.set_embedding(&id, &axis_vec(0, 1.0)).unwrap();
    assert_eq!(store.fts_search("secret tokens", 5).unwrap().len(), 1);
    assert_eq!(store.knn(&axis_vec(0, 1.0), 5).unwrap().len(), 1);
    assert!(store.purge(&id).unwrap());
    assert!(store.get(&id).is_none());
    assert!(store.fts_search("secret tokens", 5).unwrap().is_empty());
    assert!(store.knn(&axis_vec(0, 1.0), 5).unwrap().is_empty());
    assert!(store.list(true).unwrap().iter().all(|m| m.id != id));
    assert!(!store.purge(&id).unwrap(), "second purge finds nothing");
}

#[test]
fn purge_unknown_returns_false() {
    let store = Store::open(":memory:").unwrap();
    assert!(!store.purge("no-such-id").unwrap());
}

fn one_hot(i: usize) -> Vec<f32> {
    let mut v = vec![0.0f32; 768];
    v[i] = 1.0;
    v
}

fn axis_vec(i: usize, scale: f32) -> Vec<f32> {
    let mut v = vec![0.0f32; 768];
    v[i] = scale;
    v
}

#[test]
fn occurred_at_roundtrips_through_insert_get_list_and_fts() {
    let store = Store::open(":memory:").unwrap();
    let mut m = item("the march release shipped on the fifteenth");
    m.occurred_at = Some(1_700_000_000);
    let id = store.insert(&m).unwrap();
    assert_eq!(store.get(&id).unwrap().occurred_at, Some(1_700_000_000));
    assert_eq!(
        store.list(false).unwrap()[0].occurred_at,
        Some(1_700_000_000)
    );
    let hits = store.fts_search("march release", 5).unwrap();
    assert_eq!(hits[0].0.occurred_at, Some(1_700_000_000));
    // absent stays absent
    let plain = store.insert(&item("no event time")).unwrap();
    assert_eq!(store.get(&plain).unwrap().occurred_at, None);
}

/// A pre-occurred_at database must gain the column on open, not fail.
#[test]
fn open_migrates_a_database_without_the_occurred_at_column() {
    let dir = std::env::temp_dir().join(format!("remem-migrate-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("old.db");
    let _ = std::fs::remove_file(&path);
    let legacy_id;
    {
        let store = Store::open(path.to_str().unwrap()).unwrap();
        legacy_id = store.insert(&item("legacy row")).unwrap();
        store
            .connection()
            .execute_batch("ALTER TABLE memories DROP COLUMN occurred_at")
            .unwrap();
    }
    let store = Store::open(path.to_str().unwrap()).unwrap();
    assert_eq!(store.get(&legacy_id).unwrap().occurred_at, None);
    let mut m = item("backfilled row");
    m.occurred_at = Some(42);
    let id = store.insert(&m).unwrap();
    assert_eq!(store.get(&id).unwrap().occurred_at, Some(42));
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{}", path.display(), suffix));
    }
}
