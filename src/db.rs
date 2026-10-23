//! SQLite connection with sqlite-vec + graphqlite loaded, schema applied.

use crate::embed::DIMS;
use crate::error::Result;
use rusqlite::OptionalExtension;
use std::path::Path;
use std::sync::Once;

/// v3: relation polarity/provenance. Migrations are an explicit ladder below.
///
/// The version lives in `schema_meta`, not `PRAGMA user_version`: graphqlite
/// owns that pragma (it stamps 1 for its own schema and its cypher executor
/// refuses to start on any other value).
pub const SCHEMA_VERSION: i64 = 3;

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
            // Never chmod a directory this process did not create: `--db /tmp/x.db`
            // must not change /tmp, and `--db ./x.db` must not change the checkout.
            let created = !dir.exists();
            std::fs::create_dir_all(dir)?;
            #[cfg(unix)]
            if created {
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
            // SQLite creates -wal/-shm 0644; they hold full memory text.
            for suffix in ["-wal", "-shm"] {
                let mut side = path.as_os_str().to_os_string();
                side.push(suffix);
                let side = std::path::PathBuf::from(side);
                if side.exists() {
                    let _ = std::fs::set_permissions(side, std::fs::Permissions::from_mode(0o600));
                }
            }
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
        // Switching a brand-new file to WAL ignores busy_timeout, so two
        // processes opening it at once need an explicit retry.
        let mut tries = 0;
        loop {
            match conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get::<_, String>(0)) {
                Err(rusqlite::Error::SqliteFailure(e, _))
                    if e.code == rusqlite::ErrorCode::DatabaseBusy && tries < 100 =>
                {
                    tries += 1;
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                r => {
                    r?;
                    break;
                }
            }
        }
        conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA synchronous=NORMAL;")?;
        let g = graphqlite::Connection::from_rusqlite(conn)?;
        let db = Db { g };
        // graphqlite stamps `user_version=1` when it creates its own schema in
        // a fresh file, so presence of our tables, not the number alone, says
        // whether this database was ever initialised.
        if !db.table_exists("memories")? {
            db.create_schema()?;
            return Ok(db);
        }
        let v = db.schema_version()?;
        if v > SCHEMA_VERSION {
            return Err(crate::err!(
                "database schema v{v} is newer than this neocortex (v{SCHEMA_VERSION})"
            ));
        }
        // 0 = a v1 database (no schema_meta row yet); 1 = stamped v1.
        if v < 2 {
            db.migrate_v1_to_v2()?;
        }
        if v < 3 {
            db.migrate_v2_to_v3()?;
        }
        Ok(db)
    }

    fn table_exists(&self, name: &str) -> Result<bool> {
        let n: i64 = self.sql().query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
            [name],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    /// Fresh database: create every table, then stamp the version.
    fn create_schema(&self) -> Result<()> {
        self.sql().execute_batch(include_str!("schema.sql"))?;
        self.seed_predicates()?;
        self.set_version(SCHEMA_VERSION)
    }

    fn seed_predicates(&self) -> Result<()> {
        for name in [
            "lives in",
            "resides in",
            "based in",
            "work at",
            "work for",
            "employ by",
            "married to",
            "reports to",
            "born in",
            "born on",
            "headquartered in",
        ] {
            let canonical = crate::text::norm_predicate(name);
            self.sql().execute(
                "INSERT OR IGNORE INTO predicates(canonical,exclusive) VALUES (?1,1)",
                [&canonical],
            )?;
            let id: i64 = self.sql().query_row(
                "SELECT id FROM predicates WHERE canonical=?1",
                [&canonical],
                |r| r.get(0),
            )?;
            self.sql().execute(
                "INSERT OR IGNORE INTO predicate_aliases(alias_norm,predicate_id) VALUES (?1,?2)",
                rusqlite::params![canonical, id],
            )?;
        }
        Ok(())
    }

    fn migrate_v2_to_v3(&self) -> Result<()> {
        self.write(|db| {
            if !db.column_exists("relations", "polarity")? {
                db.sql().execute_batch(
                    "ALTER TABLE relations ADD COLUMN polarity INTEGER NOT NULL DEFAULT 1;",
                )?;
            }
            if !db.column_exists("relations", "ended_by")? {
                db.sql()
                    .execute_batch("ALTER TABLE relations ADD COLUMN ended_by INTEGER;")?;
            }
            db.seed_predicates()?;
            db.set_version(3)
        })
    }

    fn column_exists(&self, table: &str, column: &str) -> Result<bool> {
        Ok(self.sql().query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name=?2)",
            rusqlite::params![table, column],
            |r| r.get(0),
        )?)
    }

    fn schema_version(&self) -> Result<i64> {
        if !self.table_exists("schema_meta")? {
            return Ok(0);
        }
        let v: Option<i64> = self
            .sql()
            .query_row("SELECT version FROM schema_meta LIMIT 1", [], |r| r.get(0))
            .optional()?;
        Ok(v.unwrap_or(0))
    }

    fn set_version(&self, v: i64) -> Result<()> {
        self.sql()
            .execute_batch("CREATE TABLE IF NOT EXISTS schema_meta (version INTEGER NOT NULL)")?;
        self.sql().execute("DELETE FROM schema_meta", [])?;
        self.sql()
            .execute("INSERT INTO schema_meta(version) VALUES(?1)", [v])?;
        Ok(())
    }

    /// v1 -> v2: collapse live duplicate content hashes (keeping the oldest,
    /// retiring the rest as superseded) and add the unique live-hash index.
    /// One transaction: a crash leaves a v1 database, never a half-migrated one.
    fn migrate_v1_to_v2(&self) -> Result<()> {
        let now = crate::temporal::Clock::system().now;
        self.write(|db| {
            // Same statement predicate twice: first the vec `live` flags, then
            // the rows themselves (the MIN stays the oldest id either way).
            let dup_sql = "SELECT id FROM memories WHERE expired_at IS NULL
                 AND id <> (SELECT MIN(m2.id) FROM memories m2
                            WHERE m2.content_hash = memories.content_hash
                              AND m2.expired_at IS NULL)";
            let dups: Vec<i64> = {
                let mut st = db.sql().prepare(dup_sql)?;
                let rows = st.query_map([], |r| r.get::<_, i64>(0))?;
                rows.collect::<rusqlite::Result<_>>()?
            };
            for id in &dups {
                db.sql()
                    .execute("UPDATE memories_vec SET live=0 WHERE rowid=?1", [id])?;
            }
            db.sql().execute(
                "UPDATE memories SET expired_at=?1, superseded_by=(
                     SELECT MIN(m2.id) FROM memories m2
                     WHERE m2.content_hash = memories.content_hash
                       AND m2.expired_at IS NULL)
                 WHERE expired_at IS NULL
                   AND id <> (SELECT MIN(m2.id) FROM memories m2
                              WHERE m2.content_hash = memories.content_hash
                                AND m2.expired_at IS NULL)",
                [now],
            )?;
            db.sql()
                .execute_batch("DROP INDEX IF EXISTS idx_memories_hash")?;
            db.sql().execute_batch(
                "CREATE UNIQUE INDEX idx_memories_hash ON memories(content_hash)
                 WHERE expired_at IS NULL",
            )?;
            db.sql().execute_batch(
                "CREATE TABLE IF NOT EXISTS schema_meta (version INTEGER NOT NULL)",
            )?;
            db.set_version(2)
        })
    }

    /// Plain SQL access (same connection as Cypher).
    pub fn sql(&self) -> &rusqlite::Connection {
        self.g.sqlite_connection()
    }

    /// Cypher access (graphqlite) on the same connection.
    pub fn graph(&self) -> &graphqlite::Connection {
        &self.g
    }

    /// Run `f` inside BEGIN IMMEDIATE .. COMMIT; rolls back on error (including
    /// a failed COMMIT). A call already inside a transaction joins it, so an
    /// importer can wrap many writes in one atomic unit.
    pub fn write<T>(&self, f: impl FnOnce(&Db) -> Result<T>) -> Result<T> {
        if !self.sql().is_autocommit() {
            return f(self);
        }
        self.sql().execute_batch("BEGIN IMMEDIATE")?;
        match f(self) {
            Ok(v) => match self.sql().execute_batch("COMMIT") {
                Ok(()) => Ok(v),
                Err(e) => {
                    let _ = self.sql().execute_batch("ROLLBACK");
                    Err(e.into())
                }
            },
            Err(e) => {
                let _ = self.sql().execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }
}

/// True for a UNIQUE constraint violation (not any other constraint).
pub fn is_unique_violation(e: &rusqlite::Error) -> bool {
    matches!(
        e,
        rusqlite::Error::SqliteFailure(f, _)
            if f.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
    )
}

/// f32 slice as the little-endian blob vec0 expects.
pub fn vec_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// Inverse of [`vec_blob`]; None when the blob is not one DIMS-long vector.
pub fn blob_to_vec(blob: &[u8]) -> Option<Vec<f32>> {
    if blob.len() != DIMS * 4 {
        return None;
    }
    Some(
        blob.as_chunks::<4>()
            .0
            .iter()
            .map(|x| f32::from_le_bytes(*x))
            .collect(),
    )
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

    #[test]
    fn a_failed_commit_rolls_back_so_the_next_write_works() {
        let db = Db::open_in_memory().unwrap();
        // A deferred foreign-key violation only surfaces at COMMIT.
        let r: Result<()> = db.write(|db| {
            db.sql().execute_batch("PRAGMA defer_foreign_keys=ON")?;
            db.sql().execute(
                "INSERT INTO mentions(memory_id, entity_id) VALUES (999, 999)",
                [],
            )?;
            Ok(())
        });
        assert!(r.is_err(), "the commit must fail on the dangling mention");
        assert!(db.sql().is_autocommit(), "the transaction must be closed");
        db.write(|db| {
            db.sql()
                .execute("INSERT INTO meta(key,value) VALUES ('k','v')", [])?;
            Ok(())
        })
        .expect("the next write must not hit 'within a transaction'");
    }

    #[test]
    fn unique_live_hash_is_enforced() {
        let db = Db::open_in_memory().unwrap();
        let insert = |hash: &str| {
            db.sql().execute(
                "INSERT INTO memories(content,kind,created_at,valid_from,content_hash)
                 VALUES ('x','note',0,0,?1)",
                [hash],
            )
        };
        insert("h").unwrap();
        let e = insert("h").unwrap_err();
        assert!(is_unique_violation(&e), "{e}");
        // Retired rows do not occupy the hash.
        db.sql()
            .execute(
                "UPDATE memories SET expired_at=1 WHERE content_hash='h'",
                [],
            )
            .unwrap();
        insert("h").unwrap();
    }

    #[test]
    fn blob_to_vec_round_trips_and_rejects_wrong_lengths() {
        let v = unit(3);
        assert_eq!(blob_to_vec(&vec_blob(&v)), Some(v));
        assert_eq!(blob_to_vec(&[0u8; 4]), None);
    }
}
