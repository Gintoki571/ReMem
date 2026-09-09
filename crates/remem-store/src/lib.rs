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

    /// Existing id for this content hash, if any (includes soft-deleted rows).
    pub fn find_by_hash(&self, hash: &str) -> Option<String> {
        self.conn
            .query_row(
                "SELECT id FROM memories WHERE content_hash = ?1",
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
            "INSERT INTO memories (id, kind, content, tags, agent_id, session_id, importance, created_at, updated_at, occurred_at, deleted, content_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0, ?11)",
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
            ],
        )?;
        Ok(item.id.clone())
    }

    /// Returns None for unknown or soft-deleted ids.
    /// A corrupt row (unknown kind, unparseable tags) surfaces as Err.
    pub fn get(&self, id: &str) -> rusqlite::Result<Option<MemoryItem>> {
        self.conn
            .query_row(
                "SELECT id, kind, content, tags, agent_id, session_id, importance, created_at, updated_at, occurred_at, rowid AS m_rowid
                 FROM memories WHERE id = ?1 AND deleted = 0",
                params![id],
                row_to_item,
            )
            .optional()
    }

    /// Update content/metadata. Keeps id and created_at.
    pub fn update(&self, item: &MemoryItem) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE memories SET kind = ?2, content = ?3, tags = ?4, agent_id = ?5, session_id = ?6, importance = ?7, updated_at = ?8, occurred_at = ?9, content_hash = ?10
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

    /// Soft-delete: sets deleted = 1. get/fts_search/knn then skip the row.
    pub fn delete(&self, id: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE memories SET deleted = 1, updated_at = ?2 WHERE id = ?1 AND deleted = 0",
            params![id, MemoryItem::now()],
        )?;
        Ok(())
    }

    pub fn list(&self, include_deleted: bool) -> rusqlite::Result<Vec<MemoryItem>> {
        let sql = format!(
            "SELECT id, kind, content, tags, agent_id, session_id, importance, created_at, updated_at, occurred_at, rowid AS m_rowid
             FROM memories {} ORDER BY created_at DESC, rowid DESC",
            if include_deleted { "" } else { "WHERE deleted = 0" }
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
            "SELECT m.id, m.kind, m.content, m.tags, m.agent_id, m.session_id, m.importance, m.created_at, m.updated_at, m.occurred_at, rank, m.rowid AS m_rowid
             FROM memories_fts f
             JOIN memories m ON m.rowid = f.rowid
             WHERE memories_fts MATCH ?2 AND m.deleted = 0
             ORDER BY rank
             LIMIT ?3",
        )?;
        let rows = stmt.query_map(
            params![clamp_limit(limit), fts_quote(query), clamp_limit(limit)],
            |row| {
                let item = row_to_item(row)?;
                let rank: f64 = row.get(10)?;
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
        let mut stmt = self.conn.prepare(
            "SELECT m.id, v.distance
             FROM mem_vec v
             JOIN memories m ON m.rowid = v.rowid
             WHERE m.deleted = 0 AND v.embedding MATCH ?1 AND v.k = ?2
             ORDER BY v.distance",
        )?;
        let rows = stmt.query_map(params![serialize_f32(query), clamp_limit(k)], |row| {
            let id: String = row.get(0)?;
            let dist: f64 = row.get(1)?;
            Ok((id, dist as f32))
        })?;
        rows.collect()
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
    conn.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_memories_content_hash ON memories(content_hash)",
    )?;
    Ok(())
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
    })
}

const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "but", "by", "did", "do", "does", "for", "from",
    "had", "has", "have", "how", "i", "in", "is", "it", "no", "not", "of", "on", "or", "that",
    "the", "this", "to", "was", "what", "when", "where", "which", "who", "will", "with",
];

/// Escape a user query into a safe FTS5 one: drop English stopwords, then join
/// the remaining quoted terms with OR so filler words cannot kill the match on
/// natural-language queries. Prevents FTS5 syntax errors and column filters
/// from raw input like "foo-bar" or embedded quotes. All-stopword queries
/// fall back to the raw (still-quoted) tokens so they do not match everything.
fn fts_quote(query: &str) -> String {
    let quoted = |t: &str| format!("\"{}\"", t.replace('"', "\"\"\""));
    let tokens: Vec<&str> = query.split_whitespace().collect();
    let kept: Vec<String> = tokens
        .iter()
        .filter(|t| !STOPWORDS.contains(&t.to_ascii_lowercase().as_str()))
        .map(|t| quoted(t))
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
        assert_eq!(fts_quote("what is the parser"), "\"parser\"");
        assert_eq!(
            fts_quote("how do OR queries work"),
            "\"queries\" OR \"work\""
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
}
