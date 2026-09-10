CREATE TABLE IF NOT EXISTS memories (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  content TEXT NOT NULL,
  tags TEXT NOT NULL DEFAULT '[]',
  agent_id TEXT NOT NULL DEFAULT '',
  session_id TEXT NOT NULL DEFAULT '',
  importance REAL NOT NULL DEFAULT 0.5,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  occurred_at INTEGER,
  deleted INTEGER NOT NULL DEFAULT 0,
  content_hash TEXT,
  -- Correction chain (issue #7): the NEW row points back at the id it
  -- replaces (supersedes); superseded_at is the unix second the old row was
  -- retired. Superseded rows stay on disk for trace/audit but are hidden
  -- from every read path.
  supersedes TEXT,
  superseded_at INTEGER,
  -- Unknown-end sentinel (issue #12): 1 = "ended, date unknown", 0 = ongoing.
  ended INTEGER NOT NULL DEFAULT 0
);

-- Partial: only LIVE rows hold a content hash. A superseded row frees its
-- hash so the same content can be re-learned under a new id.
CREATE UNIQUE INDEX IF NOT EXISTS idx_memories_content_hash ON memories(content_hash)
  WHERE deleted = 0 AND superseded_at IS NULL;

-- Forget audit trail (issue #7): rowid order = insertion order, so
-- ORDER BY rowid DESC is newest-first. undo_forget consumes one event.
CREATE TABLE IF NOT EXISTS forget_events (
  id INTEGER PRIMARY KEY,
  memory_id TEXT NOT NULL,
  forgotten_at INTEGER NOT NULL
);

-- CHOICE: separate `tags` column (not concatenated content+tags) so tag words
-- match with a higher bm25 column weight (content 1.0, tags 2.0 in fts_search).
-- Porter tokenizes each column independently, so weights work with stemming.
-- Tags are extracted in-SQL from memories.tags JSON via json_each/group_concat
-- (no extra memories column, no Rust sync code); NULL/empty tags -> ''.
CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(content, tags, content='memories', content_rowid='rowid', tokenize='porter unicode61');

DROP TRIGGER IF EXISTS memories_ai;
CREATE TRIGGER memories_ai AFTER INSERT ON memories BEGIN
  INSERT INTO memories_fts(rowid, content, tags) VALUES (new.rowid, new.content, CASE WHEN json_valid(new.tags) THEN COALESCE((SELECT group_concat(value, ' ') FROM json_each(new.tags)), '') ELSE '' END);
END;
DROP TRIGGER IF EXISTS memories_ad;
CREATE TRIGGER memories_ad AFTER DELETE ON memories BEGIN
  INSERT INTO memories_fts(memories_fts, rowid, content, tags) VALUES ('delete', old.rowid, old.content, CASE WHEN json_valid(old.tags) THEN COALESCE((SELECT group_concat(value, ' ') FROM json_each(old.tags)), '') ELSE '' END);
END;
DROP TRIGGER IF EXISTS memories_au;
CREATE TRIGGER memories_au AFTER UPDATE OF content, tags ON memories BEGIN
  INSERT INTO memories_fts(memories_fts, rowid, content, tags) VALUES ('delete', old.rowid, old.content, CASE WHEN json_valid(old.tags) THEN COALESCE((SELECT group_concat(value, ' ') FROM json_each(old.tags)), '') ELSE '' END);
  INSERT INTO memories_fts(rowid, content, tags) VALUES (new.rowid, new.content, CASE WHEN json_valid(new.tags) THEN COALESCE((SELECT group_concat(value, ' ') FROM json_each(new.tags)), '') ELSE '' END);
END;

CREATE VIRTUAL TABLE IF NOT EXISTS mem_vec USING vec0(embedding FLOAT[768]);
