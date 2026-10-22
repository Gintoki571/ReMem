//! SQLite connection with sqlite-vec + graphqlite loaded, schema applied.

use crate::error::Result;
use std::path::Path;
use std::sync::Once;

pub const SCHEMA_VERSION: i64 = 1;

/// One connection for everything (relational, FTS5, vec0, Cypher), so a
/// write is one transaction.
pub struct Db {
    g: graphqlite::Connection,
}

fn register_vec() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        // sqlite-vec registers itself on every connection opened after this.
        #[allow(clippy::missing_transmute_annotations)]
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
            sqlite_vec::sqlite3_vec_init as *const (),
        )));
    });
}

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
            }
        }
        register_vec();
        let conn = rusqlite::Connection::open(path)?;
        let db = Self::init(conn)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(db)
    }

    pub fn open_in_memory() -> Result<Db> {
        register_vec();
        Self::init(rusqlite::Connection::open_in_memory()?)
    }

    fn init(conn: rusqlite::Connection) -> Result<Db> {
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        // journal_mode returns a row, so it cannot go through execute_batch.
        let _: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
        conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA synchronous=NORMAL;")?;
        let g = graphqlite::Connection::from_rusqlite(conn)?;
        let db = Db { g };
        let v: i64 = db
            .sql()
            .query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if v > SCHEMA_VERSION {
            return Err(crate::err!(
                "database schema v{v} is newer than this neocortex (v{SCHEMA_VERSION})"
            ));
        }
        db.sql().execute_batch(include_str!("schema.sql"))?;
        db.sql()
            .execute_batch(&format!("PRAGMA user_version={SCHEMA_VERSION}"))?;
        Ok(db)
    }

    /// Plain SQL access (same connection as Cypher).
    pub fn sql(&self) -> &rusqlite::Connection {
        self.g.sqlite_connection()
    }

    /// Cypher access (graphqlite) on the same connection.
    pub fn graph(&self) -> &graphqlite::Connection {
        &self.g
    }

    /// Run `f` inside BEGIN IMMEDIATE .. COMMIT; rolls back on error.
    pub fn write<T>(&self, f: impl FnOnce(&Db) -> Result<T>) -> Result<T> {
        self.sql().execute_batch("BEGIN IMMEDIATE")?;
        match f(self) {
            Ok(v) => {
                self.sql().execute_batch("COMMIT")?;
                Ok(v)
            }
            Err(e) => {
                let _ = self.sql().execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }
}

/// f32 slice as the little-endian blob vec0 expects.
pub fn vec_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(i: usize) -> Vec<f32> {
        let mut v = vec![0.0f32; 768];
        v[i] = 1.0;
        v
    }

    #[test]
    fn vec0_live_filter_and_update() {
        let db = Db::open_in_memory().unwrap();
        let c = db.sql();
        for (id, i, live) in [(1, 0, 1), (2, 0, 0), (3, 1, 1)] {
            c.execute(
                "INSERT INTO memories_vec(rowid, embedding, live) VALUES (?1, ?2, ?3)",
                rusqlite::params![id, vec_blob(&unit(i)), live],
            )
            .unwrap();
        }
        let knn = |sql: &str| -> Vec<i64> {
            let mut st = c.prepare(sql).unwrap();
            st.query_map([vec_blob(&unit(0))], |r| r.get(0))
                .unwrap()
                .map(|r| r.unwrap())
                .collect()
        };
        let q = "SELECT rowid FROM memories_vec WHERE embedding MATCH ?1 AND k = 3 AND live = 1 ORDER BY distance";
        assert_eq!(knn(q), vec![1, 3]);
        c.execute("UPDATE memories_vec SET live = 0 WHERE rowid = 1", [])
            .unwrap();
        assert_eq!(knn(q), vec![3]);
    }

    #[test]
    fn cypher_inside_transaction_rolls_back() {
        let db = Db::open_in_memory().unwrap();
        let r: Result<()> = db.write(|db| {
            db.graph()
                .cypher("CREATE (:Entity {id: 'e:1', name: 'x'})")?;
            Err(crate::err!("boom"))
        });
        assert!(r.is_err());
        let n = db
            .graph()
            .cypher("MATCH (n:Entity) RETURN count(n) AS c")
            .unwrap();
        assert_eq!(n.iter().next().unwrap().get::<i64>("c").unwrap(), 0);
        db.write(|db| {
            db.graph()
                .cypher("CREATE (:Entity {id: 'e:1', name: 'x'})")?;
            Ok(())
        })
        .unwrap();
        let n = db
            .graph()
            .cypher("MATCH (n:Entity) RETURN count(n) AS c")
            .unwrap();
        assert_eq!(n.iter().next().unwrap().get::<i64>("c").unwrap(), 1);
    }

    #[test]
    fn fts_triggers_index_memories() {
        let db = Db::open_in_memory().unwrap();
        let c = db.sql();
        c.execute("INSERT INTO memories(content, kind, created_at, valid_from, content_hash) VALUES ('production db runs Postgres', 'fact', 0, 0, 'h')", []).unwrap();
        let n: i64 = c
            .query_row(
                "SELECT count(*) FROM memories_fts WHERE memories_fts MATCH 'postgres'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
    }
}
