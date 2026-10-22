-- neocortex schema v2. Relational tables are the source of truth; the
-- graphqlite tables (nodes/edges/...) are a rebuildable projection.
-- Times are unix seconds (UTC). Validity is half-open [valid_from, valid_to).

CREATE TABLE IF NOT EXISTS memories (
  id INTEGER PRIMARY KEY,
  content TEXT NOT NULL,
  kind TEXT NOT NULL,
  tags TEXT NOT NULL DEFAULT '',          -- space-separated, folded
  importance REAL NOT NULL DEFAULT 0.5,
  scope TEXT NOT NULL DEFAULT '',
  source TEXT NOT NULL DEFAULT '',        -- writing agent
  session TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL,            -- system time: recorded
  valid_from INTEGER NOT NULL,            -- world time: became true / happened
  valid_to INTEGER,                       -- world time: stopped being true (NULL = open)
  expired_at INTEGER,                     -- system time: retired (forget / supersede)
  superseded_by INTEGER,
  access_count INTEGER NOT NULL DEFAULT 0,
  last_access INTEGER,
  content_hash TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_memories_valid ON memories(valid_from);
-- One live row per content hash: the exact-duplicate guarantee is a constraint,
-- not a read-then-insert race. Retired rows keep their history.
CREATE UNIQUE INDEX IF NOT EXISTS idx_memories_hash ON memories(content_hash)
  WHERE expired_at IS NULL;

CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
  content, tags, content='memories', content_rowid='id', tokenize='porter unicode61');
CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
  INSERT INTO memories_fts(rowid, content, tags) VALUES (new.id, new.content, new.tags);
END;
CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
  INSERT INTO memories_fts(memories_fts, rowid, content, tags) VALUES ('delete', old.id, old.content, old.tags);
END;
CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE OF content, tags ON memories BEGIN
  INSERT INTO memories_fts(memories_fts, rowid, content, tags) VALUES ('delete', old.id, old.content, old.tags);
  INSERT INTO memories_fts(rowid, content, tags) VALUES (new.id, new.content, new.tags);
END;

-- rowid = memories.id; live = expired_at IS NULL (kept in sync by the engine)
CREATE VIRTUAL TABLE IF NOT EXISTS memories_vec USING vec0(
  embedding float[768] distance_metric=cosine, live integer);

CREATE TABLE IF NOT EXISTS entities (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,                     -- display name (first seen / agent-chosen)
  norm TEXT NOT NULL UNIQUE,              -- text::norm_name(name)
  type TEXT NOT NULL DEFAULT '',
  description TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  mention_count INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS entity_aliases (
  alias_norm TEXT NOT NULL,
  entity_id INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
  origin TEXT NOT NULL DEFAULT 'agent',   -- name | agent | lexical | acronym | manual
  PRIMARY KEY (alias_norm, entity_id)
);
CREATE INDEX IF NOT EXISTS idx_alias_entity ON entity_aliases(entity_id);
CREATE VIRTUAL TABLE IF NOT EXISTS entities_fts USING fts5(name, aliases, description, tokenize='porter unicode61');
CREATE VIRTUAL TABLE IF NOT EXISTS entities_vec USING vec0(embedding float[768] distance_metric=cosine);

CREATE TABLE IF NOT EXISTS mentions (
  memory_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
  entity_id INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
  PRIMARY KEY (memory_id, entity_id)
);
CREATE INDEX IF NOT EXISTS idx_mentions_entity ON mentions(entity_id);

CREATE TABLE IF NOT EXISTS predicates (
  id INTEGER PRIMARY KEY,
  canonical TEXT NOT NULL UNIQUE,         -- text::norm_predicate
  exclusive INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS predicate_aliases (
  alias_norm TEXT PRIMARY KEY,
  predicate_id INTEGER NOT NULL REFERENCES predicates(id) ON DELETE CASCADE
);
CREATE VIRTUAL TABLE IF NOT EXISTS predicates_vec USING vec0(embedding float[768] distance_metric=cosine);

CREATE TABLE IF NOT EXISTS relations (
  id INTEGER PRIMARY KEY,
  src INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
  dst INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
  predicate_id INTEGER NOT NULL REFERENCES predicates(id),
  fact TEXT NOT NULL,                     -- natural-language statement
  weight REAL NOT NULL DEFAULT 1.0,
  evidence_count INTEGER NOT NULL DEFAULT 1,
  valid_from INTEGER NOT NULL,
  valid_to INTEGER,
  created_at INTEGER NOT NULL,
  expired_at INTEGER
);
CREATE INDEX IF NOT EXISTS idx_rel_src ON relations(src, predicate_id);
CREATE INDEX IF NOT EXISTS idx_rel_dst ON relations(dst);
CREATE TABLE IF NOT EXISTS relation_evidence (
  relation_id INTEGER NOT NULL REFERENCES relations(id) ON DELETE CASCADE,
  memory_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
  PRIMARY KEY (relation_id, memory_id)
);
CREATE INDEX IF NOT EXISTS idx_relev_memory ON relation_evidence(memory_id);
CREATE VIRTUAL TABLE IF NOT EXISTS relations_fts USING fts5(fact, predicate, tokenize='porter unicode61');
CREATE VIRTUAL TABLE IF NOT EXISTS relations_vec USING vec0(embedding float[768] distance_metric=cosine);

-- soft near-synonym links, a < b
CREATE TABLE IF NOT EXISTS similar (
  a INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
  b INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
  score REAL NOT NULL,
  PRIMARY KEY (a, b),
  CHECK (a < b)
);
CREATE INDEX IF NOT EXISTS idx_similar_b ON similar(b);

CREATE TABLE IF NOT EXISTS communities (
  entity_id INTEGER PRIMARY KEY REFERENCES entities(id) ON DELETE CASCADE,
  community INTEGER NOT NULL,
  computed_at INTEGER NOT NULL
);

-- one row: the neocortex schema version (graphqlite owns PRAGMA user_version)
CREATE TABLE IF NOT EXISTS schema_meta (
  version INTEGER NOT NULL
);

-- key/value engine state: write_generation, communities_generation, ...
CREATE TABLE IF NOT EXISTS meta (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
