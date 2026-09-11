//! SQLite-backed memory store: relational table + FTS5 index + vec0 ANN index.
use std::path::Path;
use std::sync::Once;

use remem_types::{MemoryItem, MemoryKind};
use rusqlite::ffi::sqlite3_auto_extension;
use rusqlite::{params, Connection, OptionalExtension};

const SCHEMA: &str = include_str!("../schema.sql");

static REGISTER_VEC: Once = Once::new();

/// Load the sqlite-vec extension into every future connection.
/// Idempotent; rusqlite bundles SQLite with extension support compiled in.
pub fn register_vec_extension() {
    REGISTER_VEC.call_once(|| unsafe {
        type VecInit = unsafe extern "C" fn(
            *mut rusqlite::ffi::sqlite3,
            *mut *mut i8,
            *const rusqlite::ffi::sqlite3_api_routines,
        ) -> i32;
        sqlite3_auto_extension(Some(std::mem::transmute::<*const (), VecInit>(
            sqlite_vec::sqlite3_vec_init as *const (),
        )));
    });
}

/// Upper bound for user-supplied result limits. A raw `limit as i64` can wrap
/// to negative, and SQLite reads `LIMIT -1` as "no limit" (full-table read).
pub const MAX_LIMIT: i64 = 1000;

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &str) -> rusqlite::Result<Self> {
        Self::open_path(Path::new(path))
    }

    /// Open with an explicit path (not ":memory:"). Takes `&Path` so
    /// non-UTF8 paths pass through to SQLite with no `to_str` loss.
    pub fn open_path(path: &Path) -> rusqlite::Result<Self> {
        register_vec_extension();
        let conn = Connection::open(path)?;
        // Same file as the graph connection; wait out its write transactions
        // instead of failing with SQLITE_BUSY. (rusqlite already defaults to
        // 5000 ms; set it explicitly so parity with Graph::open does not
        // depend on a library default.)
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        // Old DBs predate occurred_at/content_hash: SCHEMA creates indexes on
        // those columns, so add them BEFORE applying it (fresh DBs have no
        // memories table yet — skip those errors, SCHEMA creates it).
        for ddl in [
            "ALTER TABLE memories ADD COLUMN occurred_at INTEGER",
            "ALTER TABLE memories ADD COLUMN content_hash TEXT",
            "ALTER TABLE memories ADD COLUMN supersedes TEXT",
            "ALTER TABLE memories ADD COLUMN superseded_at INTEGER",
        ] {
            match conn.execute_batch(ddl) {
                Ok(()) => {}
                Err(e) if e.to_string().contains("no such table") => {}
                Err(e) if e.to_string().contains("duplicate column") => {}
                Err(e) => return Err(e),
            }
        }
        conn.execute_batch(SCHEMA)?;
        migrate(&conn)?;
        Ok(Self { conn })
    }

    /// Existing id for this content hash, if any. Soft-deleted and superseded
    /// rows never block dedup: their hash is freed so the same content can be
    /// re-learned under a new id.
    pub fn find_by_hash(&self, hash: &str) -> Option<String> {
        self.conn
            .query_row(
                &format!(
                    "SELECT id FROM memories WHERE content_hash = ?1 AND {}",
                    Self::live_filter("memories")
                ),
                params![hash],
                |r| r.get(0),
            )
            .optional()
            .ok()
            .flatten()
    }

    /// Insert, or return the existing id when kind+normalized content matches.
    pub fn insert(&self, item: &MemoryItem) -> rusqlite::Result<String> {
        let hash = content_hash(&item.kind, &item.content);
        if let Some(existing) = self.find_by_hash(&hash) {
            return Ok(existing);
        }
        self.conn.execute(
            "INSERT INTO memories (id, kind, content, tags, agent_id, session_id, importance, created_at, updated_at, occurred_at, deleted, content_hash, ended)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0, ?11, ?12)",
            params![
                item.id,
                item.kind.as_str(),
                item.content,
                serde_json::to_string(&item.tags).expect("tags serialize"),
                item.agent_id,
                item.session_id,
                MemoryItem::clamp_importance(item.importance) as f64,
                item.created_at,
                item.updated_at,
                item.occurred_at,
                hash,
                item.ended,
            ],
        )?;
        Ok(item.id.clone())
    }

    /// Returns None for unknown or soft-deleted ids.
    /// A corrupt row (unknown kind, unparseable tags) surfaces as Err.
    pub fn get(&self, id: &str) -> rusqlite::Result<Option<MemoryItem>> {
        self.conn
            .query_row(
                &format!(
                    "SELECT id, kind, content, tags, agent_id, session_id, importance, created_at, updated_at, occurred_at, ended, rowid AS m_rowid
                 FROM memories WHERE id = ?1 AND {}",
                    Self::live_filter("memories")
                ),
                params![id],
                row_to_item,
            )
            .optional()
    }

    /// Fetch any row by id, including soft-deleted and superseded ones.
    /// Impact/trace paths need superseded rows: their graph nodes stay, but
    /// `get` hides them. Unknown id -> None.
    pub fn get_any(&self, id: &str) -> rusqlite::Result<Option<MemoryItem>> {
        self.conn
            .query_row(
                "SELECT id, kind, content, tags, agent_id, session_id, importance, created_at, updated_at, occurred_at, ended, rowid AS m_rowid
                 FROM memories WHERE id = ?1",
                params![id],
                row_to_item,
            )
            .optional()
    }

    /// Update content/metadata. Keeps id and created_at.
    pub fn update(&self, item: &MemoryItem) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE memories SET kind = ?2, content = ?3, tags = ?4, agent_id = ?5, session_id = ?6, importance = ?7, updated_at = ?8, occurred_at = ?9, content_hash = ?10, ended = ?11
             WHERE id = ?1",
            params![
                item.id,
                item.kind.as_str(),
                item.content,
                serde_json::to_string(&item.tags).expect("tags serialize"),
                item.agent_id,
                item.session_id,
                MemoryItem::clamp_importance(item.importance) as f64,
                item.updated_at,
                item.occurred_at,
                content_hash(&item.kind, &item.content),
                item.ended,
            ],
        )?;
        Ok(())
    }

    /// Hard-delete: removes the memories row (the `memories_ad` trigger syncs
    /// it out of FTS) and the mem_vec row. Returns false if the id is unknown.
    pub fn purge(&self, id: &str) -> rusqlite::Result<bool> {
        let rowid: Option<i64> = self
            .conn
            .query_row(
                "SELECT rowid FROM memories WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(rowid) = rowid else { return Ok(false) };
        self.conn
            .execute("DELETE FROM mem_vec WHERE rowid = ?1", params![rowid])?;
        self.conn
            .execute("DELETE FROM memories WHERE id = ?1", params![id])?;
        Ok(true)
    }

    /// Soft-delete: sets deleted = 1 and records a forget event.
    /// get/fts_search/knn then skip the row.
    pub fn delete(&self, id: &str) -> rusqlite::Result<()> {
        let n = self.conn.execute(
            "UPDATE memories SET deleted = 1, updated_at = ?2 WHERE id = ?1 AND deleted = 0",
            params![id, MemoryItem::now()],
        )?;
        if n > 0 {
            self.conn.execute(
                "INSERT INTO forget_events (memory_id, forgotten_at) VALUES (?1, ?2)",
                params![id, MemoryItem::now()],
            )?;
        }
        Ok(())
    }

    /// Correction chain (issue #7): in ONE transaction, mark the old row
    /// superseded (still on disk, hidden from every read path) and insert the
    /// replacement with `supersedes` pointing back at the old id. No FOREIGN
    /// KEY: the chain is walked in Rust (trace), so hostile data cannot trap
    /// inserts. Errors when the old id is unknown or already superseded.
    pub fn supersede(&self, old_id: &str, replacement: &MemoryItem) -> rusqlite::Result<String> {
        let tx = self.conn.unchecked_transaction()?;
        let superseded = tx.execute(
            &format!(
                "UPDATE memories SET superseded_at = ?2 WHERE id = ?1 AND {}",
                Self::live_filter("memories")
            ),
            params![old_id, MemoryItem::now()],
        )?;
        if superseded == 0 {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "cannot supersede '{old_id}': unknown, deleted, or already superseded"
            )));
        }
        tx.execute(
            "INSERT INTO memories (id, kind, content, tags, agent_id, session_id, importance, created_at, updated_at, occurred_at, deleted, content_hash, supersedes, ended)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0, ?11, ?12, ?13)",
            params![
                replacement.id,
                replacement.kind.as_str(),
                replacement.content,
                serde_json::to_string(&replacement.tags).expect("tags serialize"),
                replacement.agent_id,
                replacement.session_id,
                MemoryItem::clamp_importance(replacement.importance) as f64,
                replacement.created_at,
                replacement.updated_at,
                replacement.occurred_at,
                content_hash(&replacement.kind, &replacement.content),
                old_id,
                replacement.ended,
            ],
        )?;
        tx.commit()?;
        Ok(replacement.id.clone())
    }

    /// Full correction chain, oldest -> newest. The `supersedes` pointer lives
    /// on the NEW row and points at the id it replaces, so the backward walk
    /// (toward older versions) follows the row's own pointer with a HashSet
    /// guard, and the forward walk (toward newer versions) uses the inverse
    /// lookup `SELECT id WHERE supersedes = ?` with a chain-membership guard.
    /// Both guards terminate hand-edited cycles. Unknown id -> empty chain.
    pub fn trace(&self, id: &str) -> rusqlite::Result<Vec<String>> {
        let known: Option<i64> = self
            .conn
            .query_row("SELECT 1 FROM memories WHERE id = ?1", params![id], |r| {
                r.get(0)
            })
            .optional()?;
        if known.is_none() {
            return Ok(Vec::new());
        }
        // Backward: id, its predecessor, ... (newest -> oldest walk order).
        let mut back: Vec<String> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut cur = id.to_string();
        loop {
            if !seen.insert(cur.clone()) {
                break;
            }
            back.push(cur.clone());
            let prev: Option<String> = self
                .conn
                .query_row(
                    "SELECT supersedes FROM memories WHERE id = ?1",
                    params![cur],
                    |r| r.get(0),
                )
                .optional()?
                .flatten();
            match prev {
                Some(p) => cur = p,
                None => break,
            }
        }
        // The oldest end heads the chain; forward walk appends newer versions.
        let mut chain: Vec<String> = back.into_iter().rev().collect();
        cur = id.to_string();
        loop {
            let next: Option<String> = self
                .conn
                .query_row(
                    "SELECT id FROM memories WHERE supersedes = ?1",
                    params![cur],
                    |r| r.get(0),
                )
                .optional()?
                .flatten();
            match next {
                Some(n) if !chain.contains(&n) => {
                    chain.push(n.clone());
                    cur = n;
                }
                _ => break,
            }
        }
        Ok(chain)
    }

    /// Recorded forget events, newest first. `limit` clamps at MAX_LIMIT.
    pub fn forget_events(&self, limit: usize) -> rusqlite::Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT memory_id FROM forget_events ORDER BY rowid DESC LIMIT ?1")?;
        let rows = stmt.query_map(params![clamp_limit(limit)], |r| r.get(0))?;
        rows.collect()
    }

    /// Undo one forget: revive the memory and consume its newest event.
    /// Unknown id or already-reverted event -> error.
    pub fn undo_forget(&self, id: &str) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE memories SET deleted = 0, updated_at = ?2 WHERE id = ?1 AND deleted = 1",
            params![id, MemoryItem::now()],
        )?;
        let rows = tx.changes();
        if rows == 0 {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "cannot undo forget of '{id}': unknown or not deleted"
            )));
        }
        // Consume the newest event for this id; its absence after a successful
        // revive means the ledger was tampered with — surface, don't ignore.
        let consumed = tx.execute(
            "DELETE FROM forget_events WHERE memory_id = ?1 AND id = (
               SELECT MAX(id) FROM forget_events WHERE memory_id = ?1)",
            params![id],
        )?;
        if consumed == 0 {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "no forget event recorded for '{id}'"
            )));
        }
        tx.commit()?;
        Ok(())
    }

    pub fn list(&self, include_deleted: bool) -> rusqlite::Result<Vec<MemoryItem>> {
        // Superseded rows are never listed, not even with include_deleted:
        // superseding is not deleting, and the chain stays reachable via trace.
        // include_deleted bypasses only the deleted check; superseded rows
        // are never listed (chain reachable via trace).
        let sql = format!(
            "SELECT id, kind, content, tags, agent_id, session_id, importance, created_at, updated_at, occurred_at, ended, rowid AS m_rowid
             FROM memories WHERE superseded_at IS NULL {} ORDER BY created_at DESC, rowid DESC",
            if include_deleted { "" } else { "AND deleted = 0" }
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], row_to_item)?;
        rows.collect()
    }

    /// FTS5 match with bm25 rank. More negative = better match.
    pub fn fts_search(
        &self,
        query: &str,
        limit: usize,
    ) -> rusqlite::Result<Vec<(MemoryItem, f32)>> {
        let mut stmt = self.conn.prepare(
            &format!(
                "SELECT m.id, m.kind, m.content, m.tags, m.agent_id, m.session_id, m.importance, m.created_at, m.updated_at, m.occurred_at, m.ended, bm25(memories_fts, 1.0, 2.0) AS rank, m.rowid AS m_rowid
             FROM memories_fts f
             JOIN memories m ON m.rowid = f.rowid
             WHERE memories_fts MATCH ?2 AND {}
             ORDER BY rank
             LIMIT ?3",
                Self::live_filter("m")
            ),
        )?;
        let rows = stmt.query_map(
            params![clamp_limit(limit), fts_quote(query), clamp_limit(limit)],
            |row| {
                let item = row_to_item(row)?;
                // m.ended sits at index 10; bm25 rank moved to 11.
                let rank: f64 = row.get(11)?;
                Ok((item, rank as f32))
            },
        )?;
        rows.collect()
    }

    /// Store (or overwrite) the embedding for a memory. 768 dims required.
    pub fn set_embedding(&self, id: &str, vec: &[f32]) -> rusqlite::Result<()> {
        check_dims(vec.len())?;
        let rowid: i64 = self
            .conn
            .query_row(
                "SELECT rowid FROM memories WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| {
                rusqlite::Error::InvalidParameterName(format!("unknown memory id: {id}"))
            })?;
        // vec0 has no UPSERT; delete-then-insert.
        self.conn
            .execute("DELETE FROM mem_vec WHERE rowid = ?1", params![rowid])?;
        self.conn.execute(
            "INSERT INTO mem_vec(rowid, embedding) VALUES (?1, ?2)",
            params![rowid, serialize_f32(vec)],
        )?;
        Ok(())
    }

    /// k nearest by L2 distance. Skips memories with no embedding or soft-deleted.
    /// k > stored vectors returns all of them.
    pub fn knn(&self, query: &[f32], k: usize) -> rusqlite::Result<Vec<(String, f32)>> {
        check_dims(query.len())?;
        let mut stmt = self.conn.prepare(&format!(
            "SELECT m.id, v.distance
             FROM mem_vec v
             JOIN memories m ON m.rowid = v.rowid
             WHERE {} AND v.embedding MATCH ?1 AND v.k = ?2
             ORDER BY v.distance",
            Self::live_filter("m")
        ))?;
        let rows = stmt.query_map(params![serialize_f32(query), clamp_limit(k)], |row| {
            let id: String = row.get(0)?;
            let dist: f64 = row.get(1)?;
            Ok((id, dist as f32))
        })?;
        rows.collect()
    }

    /// Central live-row predicate (issue #11): a row is visible on read
    /// paths only when it is not soft-deleted and not superseded. Used at six
    /// sites: find_by_hash, get, supersede's guard UPDATE, list, fts_search,
    /// knn. Deliberately NOT used by trace (must walk superseded chain rows),
    /// purge/set_embedding (raw rowid lookups must reach any row), delete /
    /// undo_forget (state transitions on `deleted` itself).
    fn live_filter(alias: &str) -> String {
        format!("{alias}.deleted = 0 AND {alias}.superseded_at IS NULL")
    }

    /// Escape hatch for graph/cypher coexistence checks; not part of the store API.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }
}

/// Add columns introduced after a database file was first created.
/// `occurred_at` is nullable, so backfilling is a no-op: old rows read as None.
fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let has_col = |name: &str| -> rusqlite::Result<bool> {
        let mut stmt =
            conn.prepare("SELECT 1 FROM pragma_table_info('memories') WHERE name = ?1")?;
        let present: Option<i64> = stmt.query_row(params![name], |r| r.get(0)).optional()?;
        Ok(present.is_some())
    };
    if !has_col("occurred_at")? {
        conn.execute_batch("ALTER TABLE memories ADD COLUMN occurred_at INTEGER")?;
    }
    if !has_col("content_hash")? {
        conn.execute_batch("ALTER TABLE memories ADD COLUMN content_hash TEXT")?;
    }
    // Correction chain (issue #7): legacy DBs have a FULL unique index on
    // content_hash, which would block re-learning superseded content. Detect
    // it in sqlite_master and replace it with the partial one (WHERE deleted
    // = 0 AND superseded_at IS NULL). Fresh DBs get the partial index from
    // SCHEMA, so no-op there.
    let idx_sql: Option<String> = conn
        .prepare("SELECT sql FROM sqlite_master WHERE type = 'index' AND name = 'idx_memories_content_hash'")?
        .query_row([], |r| r.get(0))
        .optional()?;
    let is_partial = idx_sql
        .as_deref()
        .map(|s| s.contains("superseded_at"))
        .unwrap_or(true);
    if !is_partial {
        conn.execute_batch(
            "DROP INDEX IF EXISTS idx_memories_content_hash;
             CREATE UNIQUE INDEX idx_memories_content_hash ON memories(content_hash)
               WHERE deleted = 0 AND superseded_at IS NULL;",
        )?;
    }
    // Unknown-end sentinel (issue #12): legacy DBs lack the column.
    if !has_col("ended")? {
        conn.execute_batch("ALTER TABLE memories ADD COLUMN ended INTEGER NOT NULL DEFAULT 0")?;
    }
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS forget_events (
           id INTEGER PRIMARY KEY,
           memory_id TEXT NOT NULL,
           forgotten_at INTEGER NOT NULL
         );",
    )?;
    // Backfill content_hash for pre-existing rows (docs/migrate-gap.md):
    // old DBs predate the column, so those rows are NULL and invisible to
    // find_by_hash dedup (SQLite unique indexes permit multiple NULLs).
    // Rust-side loop reusing content_hash(); corrupt-kind rows are skipped.
    let nulls: Vec<(String, String, String)> = conn
        .prepare("SELECT id, kind, content FROM memories WHERE content_hash IS NULL")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (id, kind_str, content) in &nulls {
        if let Some(kind) = MemoryKind::parse(kind_str) {
            let hash = content_hash(&kind, content);
            match conn.execute(
                "UPDATE memories SET content_hash = ?1 WHERE id = ?2",
                params![hash, id],
            ) {
                Ok(_) => {}
                Err(e) if e.to_string().contains("UNIQUE constraint") => {}
                Err(e) => return Err(e),
            }
        }
    }
    // FTS tags migration: old DBs have a 1-column memories_fts(content).
    // Rebuild as 2-column (content, tags) and repopulate from memories.tags.
    // (open_path already ran SCHEMA, which installs the new triggers on the
    // old 1-column table; drop them before the rebuild, recreate after.)
    let has_tags: Option<i64> = conn
        .prepare("SELECT 1 FROM pragma_table_info('memories_fts') WHERE name = 'tags'")?
        .query_row([], |r| r.get(0))
        .optional()?;
    if has_tags.is_none() {
        conn.execute_batch(
            "DROP TRIGGER IF EXISTS memories_ai;
             DROP TRIGGER IF EXISTS memories_ad;
             DROP TRIGGER IF EXISTS memories_au;
             DROP TABLE IF EXISTS memories_fts;
             CREATE VIRTUAL TABLE memories_fts USING fts5(content, tags, content='memories', content_rowid='rowid', tokenize='porter unicode61');
             INSERT INTO memories_fts(rowid, content, tags)
               SELECT rowid, content, CASE WHEN json_valid(memories.tags) THEN COALESCE((SELECT group_concat(value, ' ') FROM json_each(memories.tags)), '') ELSE '' END FROM memories;
             CREATE TRIGGER memories_ai AFTER INSERT ON memories BEGIN
               INSERT INTO memories_fts(rowid, content, tags) VALUES (new.rowid, new.content, CASE WHEN json_valid(new.tags) THEN COALESCE((SELECT group_concat(value, ' ') FROM json_each(new.tags)), '') ELSE '' END);
             END;
             CREATE TRIGGER memories_ad AFTER DELETE ON memories BEGIN
               INSERT INTO memories_fts(memories_fts, rowid, content, tags) VALUES ('delete', old.rowid, old.content, CASE WHEN json_valid(old.tags) THEN COALESCE((SELECT group_concat(value, ' ') FROM json_each(old.tags)), '') ELSE '' END);
             END;
             CREATE TRIGGER memories_au AFTER UPDATE OF content, tags ON memories BEGIN
               INSERT INTO memories_fts(memories_fts, rowid, content, tags) VALUES ('delete', old.rowid, old.content, CASE WHEN json_valid(old.tags) THEN COALESCE((SELECT group_concat(value, ' ') FROM json_each(old.tags)), '') ELSE '' END);
               INSERT INTO memories_fts(rowid, content, tags) VALUES (new.rowid, new.content, CASE WHEN json_valid(new.tags) THEN COALESCE((SELECT group_concat(value, ' ') FROM json_each(new.tags)), '') ELSE '' END);
             END;",
        )?;
    }
    Ok(())
}

/// One recorded corroboration merge (issue #14): the absorbed phrasing and
/// when it was merged into the survivor.
#[derive(Debug, Clone, PartialEq)]
pub struct MergedRow {
    pub survivor_id: String,
    pub absorbed_content: String,
    pub absorbed_at: i64,
}

impl Store {
    /// Record a corroboration merge: `content` was absorbed into survivor
    /// `survivor_id` at unix second `at` (issue #14).
    pub fn insert_merge(&self, survivor_id: &str, content: &str, at: i64) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO merges (survivor_id, absorbed_content, absorbed_at) VALUES (?1, ?2, ?3)",
            params![survivor_id, content, at],
        )?;
        Ok(())
    }

    /// Merge rows recorded against a survivor, oldest first.
    pub fn merges_for(&self, survivor_id: &str) -> rusqlite::Result<Vec<MergedRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT survivor_id, absorbed_content, absorbed_at FROM merges
             WHERE survivor_id = ?1 ORDER BY absorbed_id",
        )?;
        let rows = stmt
            .query_map(params![survivor_id], |r| {
                Ok(MergedRow {
                    survivor_id: r.get(0)?,
                    absorbed_content: r.get(1)?,
                    absorbed_at: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// FTS match over recorded absorbed phrasings, returning (survivor_id,
    /// bm25 rank). More negative = better. Same porter tokenizer as
    /// memories_fts, so stemming matches the main index.
    pub fn fts_search_merges(
        &self,
        query: &str,
        limit: usize,
    ) -> rusqlite::Result<Vec<(String, f32)>> {
        let mut stmt = self.conn.prepare(
            "SELECT survivor_id, bm25(merges_fts) AS rank FROM merges_fts
             WHERE merges_fts MATCH ?1 ORDER BY rank LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![fts_quote(query), clamp_limit(limit)], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)? as f32))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Absorbed phrasings under a survivor, oldest first (trace view).
    pub fn merge_trace(&self, survivor_id: &str) -> rusqlite::Result<Vec<String>> {
        Ok(self
            .merges_for(survivor_id)?
            .into_iter()
            .map(|m| m.absorbed_content)
            .collect())
    }
}

/// sha256(kind + normalized content). Normalization: trim, collapse
/// whitespace runs to one space, lowercase.
pub fn content_hash(kind: &MemoryKind, content: &str) -> String {
    use sha2::{Digest, Sha256};
    let normalized: String = content
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let mut hasher = Sha256::new();
    hasher.update(kind.as_str().as_bytes());
    hasher.update(b"\n");
    hasher.update(normalized.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn row_to_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryItem> {
    let id: String = row.get(0)?;
    let kind_str: String = row.get(1)?;
    // m_rowid is selected last by get/list/fts_search callers; absent in ad-hoc queries.
    let rowid: Option<i64> = row.get("m_rowid").ok().flatten();
    let who = match rowid {
        Some(r) => format!("id '{id}' (rowid {r})"),
        None => format!("id '{id}'"),
    };
    let conv_err = |idx: usize, msg: String| {
        rusqlite::Error::FromSqlConversionFailure(
            idx,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, msg)),
        )
    };
    let kind = MemoryKind::parse(&kind_str)
        .ok_or_else(|| conv_err(1, format!("unknown kind '{kind_str}' for memory {who}")))?;
    // tags is NOT NULL with a '[]' default, but tolerate NULL (legacy/foreign rows).
    let tags_json: Option<String> = row.get(3)?;
    let tags: Vec<String> = match tags_json {
        None => Vec::new(),
        Some(s) => serde_json::from_str(&s)
            .map_err(|e| conv_err(3, format!("unparseable tags JSON for memory {who}: {e}")))?,
    };
    Ok(MemoryItem {
        id,
        kind,
        content: row.get(2)?,
        tags,
        agent_id: row.get(4)?,
        session_id: row.get(5)?,
        // SQLite stores NaN REALs as NULL, so a hostile/legacy row can be
        // NULL or out of range here; clamp back to 0.0..=1.0 (NULL/NaN -> 0.5).
        importance: MemoryItem::clamp_importance(
            row.get::<_, Option<f64>>(6)?.unwrap_or(0.5) as f32
        ),
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
        occurred_at: row.get(9)?,
        // Legacy rows (column added after first release) read as ongoing.
        ended: row.get::<_, Option<bool>>(10)?.unwrap_or(false),
    })
}

const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "but", "by", "did", "do", "does", "for", "from",
    "had", "has", "have", "how", "i", "in", "is", "it", "no", "not", "of", "on", "or", "that",
    "the", "this", "to", "was", "what", "when", "where", "which", "who", "will", "with",
];

/// Minimum token length for the hybrid prefix arm (docs/stemmer-analysis.md
/// sec. 5). Gate at 5: with arms on every surviving token the three
/// adversarial eval queries match 9 junk docs (Q39 alone matches 6, via
/// `"work"*` hitting the worker/workflow stems); gated they match 0.
const PREFIX_MIN_LEN: usize = 5;
/// Prefix arm length. 4 spans the stem-group gaps (audi* -> audit, deci* ->
/// decid/decis) without the junk a 3-char arm would attract.
const PREFIX_LEN: usize = 4;

/// Escape a user query into a safe FTS5 one: drop English stopwords, then join
/// the remaining quoted terms with OR so filler words cannot kill the match on
/// natural-language queries. Prevents FTS5 syntax errors and column filters
/// from raw input like "foo-bar" or embedded quotes. All-stopword queries
/// fall back to the raw (still-quoted) tokens so they do not match everything.
///
/// Hybrid arm: a surviving (non-stopword) term of at least `PREFIX_MIN_LEN`
/// chars also gets a `PREFIX_LEN`-char prefix query, `"auditors" OR "audi"*`,
/// so a stem-group mismatch (auditors vs audit) still lands a lexical hit.
/// Query-side only: no schema change, no index rebuild. The fallback path is
/// never expanded.
fn fts_quote(query: &str) -> String {
    let quoted = |t: &str| format!("\"{}\"", t.replace('"', "\"\"\""));
    let hybrid = |t: &str| {
        let exact = quoted(t);
        if t.chars().count() < PREFIX_MIN_LEN {
            return exact;
        }
        let prefix: String = t.chars().take(PREFIX_LEN).collect();
        format!("{exact} OR {}*", quoted(&prefix))
    };
    let tokens: Vec<&str> = query.split_whitespace().collect();
    let kept: Vec<String> = tokens
        .iter()
        .filter(|t| !STOPWORDS.contains(&t.to_ascii_lowercase().as_str()))
        .map(|t| hybrid(t))
        .collect();
    if kept.is_empty() {
        tokens
            .iter()
            .map(|t| quoted(t))
            .collect::<Vec<_>>()
            .join(" OR ")
    } else {
        kept.join(" OR ")
    }
}

fn clamp_limit(n: usize) -> i64 {
    // try_from first: `usize::MAX as i64` wraps to -1 = "no limit" in SQLite.
    i64::try_from(n).unwrap_or(i64::MAX).min(MAX_LIMIT)
}

fn check_dims(n: usize) -> rusqlite::Result<()> {
    if n != 768 {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "embedding must be 768 dims, got {n}"
        )));
    }
    Ok(())
}

fn serialize_f32(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

#[cfg(test)]
mod fts_quote_tests {
    use super::fts_quote;

    #[test]
    fn drops_stopwords_and_ors_content_tokens() {
        // "parser" (6 chars) gets the prefix arm; "work" (4) does not.
        assert_eq!(fts_quote("what is the parser"), "\"parser\" OR \"pars\"*");
        assert_eq!(
            fts_quote("how do OR queries work"),
            "\"queries\" OR \"quer\"* OR \"work\""
        );
    }

    #[test]
    fn all_stopword_query_falls_back_to_quoted_tokens() {
        assert_eq!(
            fts_quote("to be or not"),
            "\"to\" OR \"be\" OR \"or\" OR \"not\""
        );
    }

    #[test]
    fn empty_query_is_empty() {
        assert_eq!(fts_quote("   "), "");
    }

    #[test]
    fn words_of_five_or_more_get_a_prefix_arm() {
        assert_eq!(fts_quote("auditors"), "\"auditors\" OR \"audi\"*");
        // exactly at the gate (5 chars)
        assert_eq!(fts_quote("audit"), "\"audit\" OR \"audi\"*");
    }

    #[test]
    fn shorter_words_are_unchanged() {
        assert_eq!(fts_quote("run db v3"), "\"run\" OR \"db\" OR \"v3\"");
    }

    #[test]
    fn gate_keeps_stopwords_unexpanded() {
        // Ungated expansion let the adversarial query "how does this work with
        // that" match 6 junk docs via work* (worker/workflow). The gate is
        // len>=5 AND non-stopword, so this stays a single bare term.
        assert_eq!(fts_quote("how does this work with that"), "\"work\"");
    }
}
