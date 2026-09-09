use remem_store::{content_hash, Store};
use remem_types::{MemoryItem, MemoryKind};
use rusqlite::{params, Connection};
use std::path::PathBuf;

fn tmp_db(name: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("remem-backfill-{}-{}.db", name, std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

// Old-schema DB: memories table predates occurred_at/content_hash columns.
fn seed_old_schema(path: &std::path::Path) {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE memories (
           id TEXT PRIMARY KEY, kind TEXT NOT NULL, content TEXT NOT NULL,
           tags TEXT NOT NULL DEFAULT '[]', agent_id TEXT NOT NULL DEFAULT '',
           session_id TEXT NOT NULL DEFAULT '', importance REAL NOT NULL DEFAULT 0.5,
           created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
           deleted INTEGER NOT NULL DEFAULT 0
         );",
    )
    .unwrap();
    for (id, content) in [
        ("old-1", "the first old memory"),
        ("old-2", "the second old memory"),
    ] {
        conn.execute(
            "INSERT INTO memories (id, kind, content, created_at, updated_at) VALUES (?1, 'note', ?2, 1, 1)",
            params![id, content],
        )
        .unwrap();
    }
}

#[test]
fn old_schema_rows_get_backfilled() {
    let path = tmp_db("old");
    seed_old_schema(&path);
    let store = Store::open_path(&path).unwrap();
    for (id, content) in [
        ("old-1", "the first old memory"),
        ("old-2", "the second old memory"),
    ] {
        let hash = content_hash(&MemoryKind::Note, content);
        assert_eq!(store.find_by_hash(&hash).as_deref(), Some(id));
    }
    let _ = std::fs::remove_file(&path);
}

#[test]
fn fresh_db_unaffected_and_reopen_idempotent() {
    let path = tmp_db("fresh");
    let s = path.to_str().unwrap().to_string();
    let store = Store::open(&s).unwrap();
    let m = MemoryItem::new(MemoryKind::Fact, "fresh content here".into());
    let id = store.insert(&m).unwrap();
    assert_eq!(
        store
            .find_by_hash(&content_hash(&m.kind, &m.content))
            .as_deref(),
        Some(id.as_str())
    );
    drop(store);
    // Re-open: backfill must not error or change anything.
    let store2 = Store::open(&s).unwrap();
    assert_eq!(
        store2
            .find_by_hash(&content_hash(&m.kind, &m.content))
            .as_deref(),
        Some(id.as_str())
    );
    let nulls: i64 = store2
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM memories WHERE content_hash IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(nulls, 0);
    let _ = std::fs::remove_file(&path);
}
