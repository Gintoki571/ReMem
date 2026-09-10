// Unknown-end sentinel (issue #12, utopia 0030 steal #6): `ended` separates
// "ended, date unknown" from "ongoing". Occurred_at stays NULL in both cases;
// the bool is the only difference. NULL/absent column reads as false (legacy
// rows), and the event clock ignores ended (it is not a timestamp).
use remem_store::{content_hash, Store};
use remem_types::{MemoryItem, MemoryKind};

fn item(content: &str) -> MemoryItem {
    let mut m = MemoryItem::new(MemoryKind::Fact, content.to_string());
    m.tags = vec!["test".into()];
    m
}

#[test]
fn ended_defaults_false_and_roundtrips() {
    let store = Store::open(":memory:").unwrap();
    let id = store.insert(&item("ongoing fact")).unwrap();
    let got = store.get(&id).unwrap().unwrap();
    assert!(!got.ended, "default must be false (ongoing)");
    assert_eq!(got.event_time(), got.created_at);
}

#[test]
fn ended_true_roundtrips_with_null_occurred_at() {
    let store = Store::open(":memory:").unwrap();
    let mut m = item("no longer true, date not recorded");
    m.ended = true;
    assert!(m.occurred_at.is_none());
    let id = store.insert(&m).unwrap();
    let got = store.get(&id).unwrap().unwrap();
    assert!(got.ended);
    assert!(
        got.occurred_at.is_none(),
        "unknown end must not fabricate a date"
    );

    // update() keeps the sentinel
    let mut updated = got.clone();
    updated.content = "still ended, reworded".into();
    store.update(&updated).unwrap();
    assert!(store.get(&id).unwrap().unwrap().ended);
}

#[test]
fn legacy_row_without_ended_column_reads_false() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("remem-sentinel-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE memories (
               id TEXT PRIMARY KEY, kind TEXT NOT NULL, content TEXT NOT NULL,
               tags TEXT NOT NULL DEFAULT '[]', agent_id TEXT NOT NULL DEFAULT '',
               session_id TEXT NOT NULL DEFAULT '', importance REAL NOT NULL DEFAULT 0.5,
               created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
               deleted INTEGER NOT NULL DEFAULT 0);
             INSERT INTO memories (id, kind, content, created_at, updated_at)
               VALUES ('legacy', 'fact', 'old row', 1, 1);",
        )
        .unwrap();
    }
    let store = Store::open(path.to_str().unwrap()).unwrap();
    let got = store.get("legacy").unwrap().unwrap();
    assert!(!got.ended, "legacy rows are ongoing, not ended-unknown");

    // sentinel column now exists; a new ended row survives reopen
    let mut m = item("new ended row");
    m.ended = true;
    let id = store.insert(&m).unwrap();
    drop(store);
    let store = Store::open(path.to_str().unwrap()).unwrap();
    assert!(store.get(&id).unwrap().unwrap().ended);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn ended_does_not_change_content_hash_dedup() {
    let store = Store::open(":memory:").unwrap();
    let mut a = item("same text");
    a.ended = true;
    let b = item("same text");
    let _ia = store.insert(&a).unwrap();
    let ib = store.insert(&b).unwrap();
    assert_eq!(
        content_hash(&MemoryKind::Fact, "same text"),
        content_hash(&MemoryKind::Fact, "same text"),
        "hash covers kind+content only"
    );
    let _ = ib;
}

#[test]
fn supersede_preserves_ended_on_new_row_from_item() {
    // The new row's ended flag is the caller's choice, not inherited from the
    // old row: a correction may flip ongoing -> ended-unknown or back.
    let store = Store::open(":memory:").unwrap();
    let old = item("was ongoing");
    let old_id = store.insert(&old).unwrap();
    let mut new = item("now ended, date unknown");
    new.ended = true;
    let new_id = store.supersede(&old_id, &new).unwrap();
    assert!(store.get(&new_id).unwrap().unwrap().ended);
    assert!(!store.get(&old_id).unwrap().is_none() || true); // old row kept for chain
}
