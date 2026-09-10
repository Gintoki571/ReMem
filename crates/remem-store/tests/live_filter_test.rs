//! Central live_filter (issue #11): `{alias}.deleted = 0 AND
//! {alias}.superseded_at IS NULL` must gate all six read paths — find_by_hash,
//! get, supersede's UPDATE, list, fts_search, knn. trace / purge /
//! set_embedding / delete / undo_forget are deliberately UNfiltered.

use remem_store::{content_hash, Store};
use remem_types::{MemoryItem, MemoryKind};

fn item(content: &str) -> MemoryItem {
    MemoryItem::new(MemoryKind::Fact, content.to_string())
}

fn one_hot(i: usize) -> Vec<f32> {
    let mut v = vec![0.0f32; 768];
    v[i] = 1.0;
    v
}

#[test]
fn deleted_rows_invisible_on_all_read_paths() {
    let store = Store::open(":memory:").unwrap();
    let id = store.insert(&item("parser runs on port 8080")).unwrap();
    store.set_embedding(&id, &one_hot(3)).unwrap();
    store.delete(&id).unwrap();

    assert!(
        store
            .find_by_hash(&content_hash(
                &item("parser runs on port 8080").kind,
                "parser runs on port 8080"
            ))
            .is_none(),
        "find_by_hash must skip deleted"
    );
    assert!(store.get(&id).unwrap().is_none(), "get must skip deleted");
    assert!(
        store.list(false).unwrap().iter().all(|m| m.id != id),
        "list must skip deleted"
    );
    assert!(
        store
            .fts_search("parser", 10)
            .unwrap()
            .iter()
            .all(|(m, _)| m.id != id),
        "fts_search must skip deleted"
    );
    assert!(
        store
            .knn(&one_hot(3), 5)
            .unwrap()
            .iter()
            .all(|(i, _)| i != &id),
        "knn must skip deleted"
    );
    // Supersede of a dead row must error, not double-write state.
    assert!(
        store
            .supersede(&id, &item("parser runs on port 9090"))
            .is_err(),
        "supersede of deleted row must error"
    );
}

#[test]
fn superseded_rows_invisible_on_all_read_paths() {
    let store = Store::open(":memory:").unwrap();
    let old = store.insert(&item("parser runs on port 8080")).unwrap();
    store.set_embedding(&old, &one_hot(4)).unwrap();
    let new_id = store
        .supersede(&old, &item("parser runs on port 9090"))
        .unwrap();

    let hash = content_hash(
        &item("parser runs on port 8080").kind,
        "parser runs on port 8080",
    );
    assert_eq!(
        store.find_by_hash(&hash),
        None,
        "find_by_hash must skip superseded"
    );
    assert!(
        store.get(&old).unwrap().is_none(),
        "get must skip superseded"
    );
    assert!(
        store.list(false).unwrap().iter().all(|m| m.id != old),
        "list must skip superseded"
    );
    // New row content differs, so an fts hit on the old wording must be gone.
    assert!(
        store
            .fts_search("8080", 10)
            .unwrap()
            .iter()
            .all(|(m, _)| m.id != old),
        "fts_search must skip superseded"
    );
    assert!(
        store
            .knn(&one_hot(4), 5)
            .unwrap()
            .iter()
            .all(|(i, _)| i != &old),
        "knn must skip superseded"
    );
    // ...and the supersede UPDATE must reject a second round on the dead row.
    assert!(
        store
            .supersede(&old, &item("parser runs on port 7777"))
            .is_err(),
        "supersede of superseded row must error"
    );
    assert!(
        store.get(&new_id).unwrap().is_some(),
        "replacement stays live"
    );
}

#[test]
fn unfiltered_paths_still_reach_dead_rows() {
    let store = Store::open(":memory:").unwrap();
    let old = store.insert(&item("parser runs on port 8080")).unwrap();
    store.set_embedding(&old, &one_hot(5)).unwrap();
    store
        .supersede(&old, &item("parser runs on port 9090"))
        .unwrap();
    store.delete(&old).unwrap();
    // trace/purge-class lookups are raw rowid/id reads: dead rows reachable.
    let n: i64 = store
        .connection()
        .query_row("SELECT count(*) FROM memories WHERE id = ?1", [&old], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(n, 1, "row still on disk for trace/purge");
}

#[test]
fn live_row_visible_on_all_read_paths() {
    let store = Store::open(":memory:").unwrap();
    let id = store.insert(&item("cache holds 256 entries")).unwrap();
    store.set_embedding(&id, &one_hot(6)).unwrap();
    assert!(store
        .find_by_hash(&content_hash(
            &item("cache holds 256 entries").kind,
            "cache holds 256 entries"
        ))
        .is_some());
    assert!(store.get(&id).unwrap().is_some());
    assert_eq!(store.list(false).unwrap().len(), 1);
    assert_eq!(store.fts_search("cache", 10).unwrap().len(), 1);
    assert_eq!(store.knn(&one_hot(6), 5).unwrap().len(), 1);
}
