//! One-shot import of a ReMem v3 SQLite file. Read-only: plain rusqlite, no
//! extensions, no virtual tables touched. Live memories are re-embedded by
//! `Engine::remember`; the old graph is not imported.

use crate::engine::{Engine, RememberInput};
use crate::error::Result;
use rusqlite::{Connection, OpenFlags};
use std::path::Path;

/// Import every live, non-superseded ReMem memory. Returns
/// `(imported, skipped)` where skipped counts deleted/superseded rows.
pub fn import_remem(engine: &Engine, path: &Path) -> Result<(usize, usize)> {
    let (inputs, skipped) = read_memories(path)?;
    let mut imported = 0;
    for inp in inputs {
        engine.remember(inp)?;
        imported += 1;
    }
    Ok((imported, skipped))
}

/// Read and map the rows without an engine, so the mapping is unit-testable.
fn read_memories(path: &Path) -> Result<(Vec<RememberInput>, usize)> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let superseded = if column_exists(&conn, "memories", "superseded_at")? {
        "superseded_at"
    } else {
        "NULL"
    };
    let ended = if column_exists(&conn, "memories", "ended")? {
        "ended"
    } else {
        "0"
    };
    let sql = format!(
        "SELECT kind, content, tags, agent_id, session_id, importance, created_at, \
         occurred_at, deleted, {superseded}, {ended} FROM memories"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    let mut skipped = 0;
    while let Some(row) = rows.next()? {
        let kind: String = row.get(0)?;
        let content: String = row.get(1)?;
        let tags_json: String = row.get(2)?;
        let agent_id: String = row.get(3)?;
        let session_id: String = row.get(4)?;
        let importance: f64 = row.get(5)?;
        let created_at: i64 = row.get(6)?;
        let occurred_at: Option<i64> = row.get(7)?;
        let deleted: i64 = row.get(8)?;
        let superseded_at: Option<i64> = row.get(9)?;
        let ended: i64 = row.get(10)?;
        let valid_from = occurred_at.unwrap_or(created_at);

        if deleted != 0 || superseded_at.is_some() || content.trim().is_empty() {
            skipped += 1;
            continue;
        }
        out.push(RememberInput {
            content,
            kind: Some(kind),
            tags: serde_json::from_str::<Vec<String>>(&tags_json).unwrap_or_default(),
            importance: Some(importance as f32),
            source: Some(agent_id).filter(|s| !s.is_empty()),
            session: Some(session_id).filter(|s| !s.is_empty()),
            valid_from: Some(valid_from.to_string()),
            // ReMem "ended, date unknown": the latest date we know it held is when it was written.
            valid_to: (ended != 0).then(|| created_at.max(valid_from + 1).to_string()),
            ..RememberInput::default()
        });
    }
    Ok((out, skipped))
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn temp_db(tag: &str) -> PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("neocortex-import-{}-{tag}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("remem.db")
    }

    #[test]
    fn maps_live_rows_and_skips_deleted_and_superseded() {
        let path = temp_db("v3");
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(
                "CREATE TABLE memories (
                   id TEXT PRIMARY KEY, kind TEXT, content TEXT, tags TEXT,
                   agent_id TEXT, session_id TEXT, importance REAL,
                   created_at INTEGER, occurred_at INTEGER,
                   deleted INTEGER DEFAULT 0, superseded_at INTEGER);
                 INSERT INTO memories VALUES ('a','fact','The staging box runs Debian 13.',
                   '[\"env\",\"linux\"]','claude','s1',0.8,1000,900,0,NULL);
                 INSERT INTO memories VALUES ('b','note','deleted row','[]','','',0.5,1000,NULL,1,NULL);
                 INSERT INTO memories VALUES ('c','fact','superseded row','[]','','',0.5,1000,NULL,0,1100);
                 INSERT INTO memories VALUES ('d','event','Deploy moved to Friday.','not json','','',0.5,2000,NULL,0,NULL);",
            )
            .unwrap();
        }

        let (rows, skipped) = read_memories(&path).unwrap();
        assert_eq!(skipped, 2);
        assert_eq!(rows.len(), 2);

        let first = &rows[0];
        assert_eq!(first.content, "The staging box runs Debian 13.");
        assert_eq!(first.kind.as_deref(), Some("fact"));
        assert_eq!(first.tags, vec!["env", "linux"]);
        assert_eq!(first.importance, Some(0.8));
        assert_eq!(first.source.as_deref(), Some("claude"));
        assert_eq!(first.session.as_deref(), Some("s1"));
        assert_eq!(first.valid_from.as_deref(), Some("900"));
        assert!(first.entities.is_empty());

        let second = &rows[1];
        assert_eq!(second.valid_from.as_deref(), Some("2000"));
        assert!(second.tags.is_empty(), "bad tags JSON degrades to none");
        assert_eq!(second.source, None, "empty agent_id is dropped");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn works_without_the_superseded_at_column() {
        let path = temp_db("v2");
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(
                "CREATE TABLE memories (
                   id TEXT PRIMARY KEY, kind TEXT, content TEXT, tags TEXT,
                   agent_id TEXT, session_id TEXT, importance REAL,
                   created_at INTEGER, occurred_at INTEGER, deleted INTEGER DEFAULT 0);
                 INSERT INTO memories VALUES ('a','note','Older file row','[]','','',0.5,7,NULL,0);",
            )
            .unwrap();
        }
        let (rows, skipped) = read_memories(&path).unwrap();
        assert_eq!(skipped, 0);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].valid_from.as_deref(), Some("7"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_file_is_an_error() {
        let path = temp_db("absent").with_file_name("nope.db");
        assert!(read_memories(&path).is_err());
    }
}
