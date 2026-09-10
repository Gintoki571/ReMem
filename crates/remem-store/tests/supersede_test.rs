//! Correction chain (issue #7): supersede, trace, forget undo, partial
//! unique content_hash index.

use std::path::PathBuf;

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

fn tmp_db(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("remem-supersede-{tag}-{}.db", std::process::id()));
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", p.display()));
    }
    p
}

#[test]
fn supersede_marks_old_and_inserts_new_in_one_tx() {
    let store = Store::open(":memory:").unwrap();
    let old = store.insert(&item("deploy uses port 8080")).unwrap();
    let new = item("deploy uses port 9090");
    let new_id = store.supersede(&old, &new).unwrap();
    assert_ne!(new_id, old);
    // Old row: still on disk, hidden from every read path.
    assert!(store.get(&old).unwrap().is_none());
    // New row is live and linked back.
    assert_eq!(store.get(&new_id).unwrap().unwrap().content, new.content);
    let (supersedes, superseded_at): (Option<String>, Option<i64>) = store
        .connection()
        .query_row(
            "SELECT supersedes, superseded_at FROM memories WHERE id = ?1",
            [old.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!(superseded_at.is_some());
    let (n_sup, s_at): (Option<String>, Option<i64>) = store
        .connection()
        .query_row(
            "SELECT supersedes, superseded_at FROM memories WHERE id = ?1",
            [new_id.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    // The NEW row points back at the id it replaces.
    assert_eq!(n_sup.as_deref(), Some(old.as_str()));
    assert!(s_at.is_none());
}

#[test]
fn supersede_unknown_or_already_superseded_errors() {
    let store = Store::open(":memory:").unwrap();
    assert!(store.supersede("no-such-id", &item("x")).is_err());
    let old = store.insert(&item("v1")).unwrap();
    store.supersede(&old, &item("v2")).unwrap();
    // Old row already superseded: a second supersede must fail, not double-write.
    assert!(store.supersede(&old, &item("v3")).is_err());
}

#[test]
fn find_by_hash_skips_superseded_and_deleted_so_content_is_relearnable() {
    let store = Store::open(":memory:").unwrap();
    let m = item("deploy uses port 8080");
    let hash = content_hash(&m.kind, &m.content);
    let old = store.insert(&m).unwrap();
    let new = item("deploy uses port 8080 REVISED");
    store.supersede(&old, &new).unwrap();
    assert_eq!(store.find_by_hash(&hash), None, "superseded row hidden");
    // Re-learning the original content after supersede gets a NEW id.
    let again = item("deploy uses port 8080");
    let id2 = store.insert(&again).unwrap();
    assert_ne!(id2, old);
    assert_eq!(store.find_by_hash(&hash).as_deref(), Some(id2.as_str()));
    // Soft-deleted rows are equally invisible to dedup.
    store.delete(&id2).unwrap();
    assert_eq!(store.find_by_hash(&hash), None);
}

#[test]
fn get_list_fts_knn_exclude_superseded() {
    let store = Store::open(":memory:").unwrap();
    let old = store.insert(&item("port 8080 deploy")).unwrap();
    let new_id = store.supersede(&old, &item("port 9090 deploy")).unwrap();
    assert!(store.get(&old).unwrap().is_none());
    let ids: Vec<String> = store
        .list(false)
        .unwrap()
        .into_iter()
        .map(|m| m.id)
        .collect();
    assert_eq!(ids, vec![new_id.clone()]);
    let fts: Vec<String> = store
        .fts_search("deploy", 10)
        .unwrap()
        .into_iter()
        .map(|(m, _)| m.id)
        .collect();
    assert_eq!(fts, vec![new_id.clone()], "fts must skip superseded");
    store.set_embedding(&new_id, &one_hot(0)).unwrap();
    let knn: Vec<String> = store
        .knn(&one_hot(0), 10)
        .unwrap()
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(knn, vec![new_id], "knn must skip superseded");
    // include_deleted still hides superseded rows: superseding is not deleting.
    assert!(store.list(true).unwrap().iter().all(|m| m.id != old));
}

#[test]
fn partial_unique_index_allows_live_duplicate_only_after_supersede() {
    let store = Store::open(":memory:").unwrap();
    let m = item("unique content here");
    let old = store.insert(&m).unwrap();
    let hash = content_hash(&m.kind, &m.content);
    // Two LIVE rows with the same hash are still rejected by the index.
    let dup = store.connection().execute(
        "INSERT INTO memories (id, kind, content, created_at, updated_at, content_hash)
         VALUES ('dup', 'fact', 'unique content here', 1, 1, ?1)",
        [hash.as_str()],
    );
    assert!(
        dup.is_err(),
        "live duplicate must violate the partial index"
    );
    store
        .supersede(&old, &item("unique content here v2"))
        .unwrap();
    // Old row is superseded: a live row with the same hash is now allowed.
    store
        .connection()
        .execute(
            "INSERT INTO memories (id, kind, content, created_at, updated_at, content_hash)
             VALUES ('dup2', 'fact', 'unique content here', 2, 2, ?1)",
            [hash.as_str()],
        )
        .expect("superseded row must free the hash");
}

#[test]
fn migration_replaces_legacy_full_unique_index_with_partial() {
    // Old DB: non-partial unique index on content_hash, one live row.
    let path = tmp_db("legacy-index");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE memories (
               id TEXT PRIMARY KEY, kind TEXT NOT NULL, content TEXT NOT NULL,
               tags TEXT NOT NULL DEFAULT '[]', agent_id TEXT NOT NULL DEFAULT '',
               session_id TEXT NOT NULL DEFAULT '', importance REAL NOT NULL DEFAULT 0.5,
               created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
               deleted INTEGER NOT NULL DEFAULT 0, content_hash TEXT);
             CREATE UNIQUE INDEX idx_memories_content_hash ON memories(content_hash);
             INSERT INTO memories (id, kind, content, created_at, updated_at, content_hash)
               VALUES ('old-1', 'fact', 'legacy content', 1, 1, 'hash-old-1');",
        )
        .unwrap();
    }
    let store = Store::open_path(&path).unwrap();
    // Hash 'hash-old-1' is held by a live row; a second live row is rejected...
    let dup = store.connection().execute(
        "INSERT INTO memories (id, kind, content, created_at, updated_at, content_hash)
         VALUES ('dup', 'fact', 'x', 1, 1, 'hash-old-1')",
        [],
    );
    assert!(dup.is_err());
    // ...but after that row is superseded the hash frees up (partial index).
    store
        .connection()
        .execute(
            "UPDATE memories SET superseded_at = 99 WHERE id = 'old-1'",
            [],
        )
        .unwrap();
    store
        .connection()
        .execute(
            "INSERT INTO memories (id, kind, content, created_at, updated_at, content_hash)
             VALUES ('new-1', 'fact', 'y', 2, 2, 'hash-old-1')",
            [],
        )
        .expect("partial index must replace the legacy full one");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn trace_walks_backward_and_forward_through_the_chain() {
    let store = Store::open(":memory:").unwrap();
    let v1 = store.insert(&item("port 8080")).unwrap();
    let v2_id = store.supersede(&v1, &item("port 9090")).unwrap();
    let v3 = store.supersede(&v2_id, &item("port 7070")).unwrap();
    // From the oldest end: forward to the newest.
    assert_eq!(
        store.trace(&v1).unwrap(),
        vec![v1.clone(), v2_id.clone(), v3.clone()]
    );
    // From the newest end: backward to the oldest.
    assert_eq!(store.trace(&v3).unwrap(), vec![v1, v2_id, v3.clone()]);
    // From the middle: both directions join into one chain.
    let mid = store.supersede(&v3, &item("port 6060")).unwrap();
    let chain = store.trace(&v3).unwrap();
    assert_eq!(chain.len(), 4);
    assert_eq!(chain[0], v1_of(&store, &chain));
    assert_eq!(chain.last().unwrap(), &mid);
}

/// The chain always starts at the memory nothing supersedes.
fn v1_of(store: &Store, chain: &[String]) -> String {
    for id in chain {
        let prev: Option<String> = store
            .connection()
            .query_row("SELECT supersedes FROM memories WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .unwrap();
        if prev.is_none() {
            return id.clone();
        }
    }
    unreachable!()
}

#[test]
fn trace_unknown_id_is_empty() {
    let store = Store::open(":memory:").unwrap();
    assert!(store.trace("no-such-id").unwrap().is_empty());
}

#[test]
fn trace_terminates_on_hand_edited_cycle() {
    let store = Store::open(":memory:").unwrap();
    let a = store.insert(&item("a")).unwrap();
    let b = store.supersede(&a, &item("b")).unwrap();
    // Hostile data: b supersedes a AND a supersedes b (cycle).
    store
        .connection()
        .execute(
            "UPDATE memories SET supersedes = ?1 WHERE id = ?2",
            rusqlite::params![b, a],
        )
        .unwrap();
    let chain = store.trace(&a).unwrap();
    assert_eq!(chain.len(), 2, "cycle guard must stop the walk: {chain:?}");
    assert!(store.trace(&b).is_ok());
}

#[test]
fn forget_events_recorded_newest_first_and_undo_revives() {
    let store = Store::open(":memory:").unwrap();
    let a = store.insert(&item("alpha")).unwrap();
    let b = store.insert(&item("beta")).unwrap();
    store.delete(&a).unwrap();
    store.delete(&b).unwrap();
    let events = store.forget_events(10).unwrap();
    assert_eq!(
        events,
        vec![b.clone(), a.clone()],
        "newest first (rowid DESC)"
    );
    store.undo_forget(&a).unwrap();
    assert!(store.get(&a).unwrap().is_some(), "undo revives");
    assert!(store.get(&b).unwrap().is_none(), "untouched stays deleted");
    // Reverting twice is an error: the forget event was consumed.
    assert!(store.undo_forget(&a).is_err());
}

#[test]
fn forget_events_survive_reopen() {
    let path = tmp_db("reopen");
    let a;
    {
        let store = Store::open_path(&path).unwrap();
        a = store.insert(&item("alpha")).unwrap();
        store.delete(&a).unwrap();
    }
    {
        let store = Store::open_path(&path).unwrap();
        assert_eq!(store.forget_events(10).unwrap(), vec![a.clone()]);
        store.undo_forget(&a).unwrap();
        assert!(store.get(&a).unwrap().is_some());
    }
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
}

#[test]
fn undo_forget_unknown_id_errors() {
    let store = Store::open(":memory:").unwrap();
    assert!(store.undo_forget("never-forgotten").is_err());
}
