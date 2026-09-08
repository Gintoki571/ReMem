//! MUST-VERIFY from SPEC: graphqlite and sqlite-vec on one connection/file.
//!
//! Both are SQLite extensions loaded into the same `rusqlite::Connection`, so
//! the graph layer can sit inside the store's database instead of a sidecar.

use remem_graph::Graph;
use remem_types::MemoryKind;
use serde_json::json;

fn register_vec() {
    unsafe {
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute::<
            *const (),
            unsafe extern "C" fn(
                *mut rusqlite::ffi::sqlite3,
                *mut *mut i8,
                *const rusqlite::ffi::sqlite3_api_routines,
            ) -> i32,
        >(
            sqlite_vec::sqlite3_vec_init as *const ()
        )));
    }
}

#[test]
fn graph_and_vec0_share_one_connection() {
    register_vec();
    let raw = rusqlite::Connection::open_in_memory().unwrap();
    raw.execute_batch(
        "CREATE TABLE memories(id TEXT PRIMARY KEY, content TEXT);
         CREATE VIRTUAL TABLE mem_vec USING vec0(rowid INTEGER PRIMARY KEY, embedding FLOAT[4]);",
    )
    .unwrap();
    raw.execute(
        "INSERT INTO memories VALUES ('m1', 'the deck is on fire')",
        [],
    )
    .unwrap();
    let rowid: i64 = raw
        .query_row("SELECT rowid FROM memories WHERE id = 'm1'", [], |r| {
            r.get(0)
        })
        .unwrap();
    let blob: Vec<u8> = [0.0f32, 1.0, 0.0, 0.0]
        .iter()
        .flat_map(|f| f.to_le_bytes())
        .collect();
    raw.execute(
        "INSERT INTO mem_vec(rowid, embedding) VALUES (?1, ?2)",
        rusqlite::params![rowid, blob],
    )
    .unwrap();

    // same connection, now also a graph
    let g = Graph::from_connection(raw).unwrap();
    g.upsert_memory("m1", MemoryKind::Event).unwrap();
    g.upsert_memory("m2", MemoryKind::Event).unwrap();
    g.link("m1", "m2", "RELATES_TO").unwrap();

    // vec0 still answers through the borrowed underlying connection
    let hits: Vec<String> = {
        let mut stmt = g
            .sqlite()
            .prepare("SELECT rowid FROM mem_vec WHERE embedding MATCH ?1 AND k = 1")
            .unwrap();
        stmt.query_map([blob], |r| r.get::<_, i64>(0))
            .unwrap()
            .map(|v| v.unwrap().to_string())
            .collect()
    };
    assert_eq!(hits, vec!["1".to_string()]);

    // and the graph answers too
    assert_eq!(g.stats().unwrap()["nodes"], json!(2));
    assert_eq!(
        g.neighbors("m1").unwrap(),
        vec![("m2".to_string(), "RELATES_TO".to_string())]
    );
}

#[test]
fn graph_and_vec0_share_one_file() {
    register_vec();
    let path = std::env::temp_dir().join(format!("remem-graph-vec-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    {
        let raw = rusqlite::Connection::open(&path).unwrap();
        raw.execute_batch("CREATE VIRTUAL TABLE IF NOT EXISTS v USING vec0(rowid INTEGER PRIMARY KEY, embedding FLOAT[768]);")
            .unwrap();
        let g = Graph::from_connection(raw).unwrap();
        g.upsert_memory("m1", MemoryKind::Fact).unwrap();
    }
    let raw = rusqlite::Connection::open(&path).unwrap();
    let g = Graph::from_connection(raw).unwrap();
    let tables: Vec<String> = g
        .sqlite()
        .prepare(
            "SELECT name FROM sqlite_master \
             WHERE type = 'table' AND (name IN ('nodes','edges') OR name LIKE 'v_%') \
             ORDER BY name",
        )
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|v| v.unwrap())
        .collect();
    assert!(tables.contains(&"nodes".to_string()), "{tables:?}");
    assert!(
        tables.iter().any(|t| t.contains("vec")),
        "vec0 tables vanished: {tables:?}"
    );
    assert_eq!(g.neighbors("m1").unwrap(), Vec::<(String, String)>::new());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn two_connections_share_one_file_without_busy_errors() {
    // remem-store keeps its own connection on this file, so the graph is a
    // second writer. Interleaved writes must wait, not fail with SQLITE_BUSY.
    let path = std::env::temp_dir().join(format!("remem-graph-two-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let a = Graph::open(&path).unwrap();
    let b = Graph::open(&path).unwrap();
    for i in 0..10 {
        a.upsert_memory(&format!("a{i}"), MemoryKind::Note).unwrap();
        b.upsert_memory(&format!("b{i}"), MemoryKind::Note).unwrap();
        a.link(&format!("a{i}"), &format!("b{i}"), "RELATES_TO")
            .unwrap();
    }
    assert_eq!(a.stats().unwrap()["nodes"], json!(20));
    assert_eq!(
        b.neighbors("a3").unwrap(),
        vec![("b3".to_string(), "RELATES_TO".to_string())]
    );
    let _ = std::fs::remove_file(&path);
}
