# ReMem v3 architecture

Source revision: f806927 (branch v3). Dirty at review time (sibling edits in flight): crates/remem-mcp/{README.md,src/main.rs,tests/mcp_stdio.rs} adding MCP `central`/`path` tools.

Six crates under `crates/`, one Cargo workspace. One SQLite file holds relational, FTS5, vec0, and graph tables side by side.

## Crate responsibilities and key APIs

- **remem-types** (`src/lib.rs`): shared data model, no I/O. `MemoryKind` (fact|decision|mistake|preference|event|note), `MemoryItem` (id, kind, content, tags, agent_id, session_id, importance, timestamps, `occurred_at` option), `RecallQuery` (text, k, filters, `max_chars` budget), `RecallHit` (item + score + reasons). `MemoryItem::event_time()` = occurred_at or created_at: the event clock for filtering and recency.

- **remem-store**: SQLite persistence. `Store::open(path)` registers sqlite-vec via `sqlite3_auto_extension` (Once), sets WAL + 5s busy timeout, adds `occurred_at`/`content_hash` columns to old DBs, applies schema.sql, then runs `migrate()`. CRUD: `insert` (dedups by content hash, returns existing id), `get` (skips soft-deleted), `update`, `delete` (soft), `purge` (hard: row + mem_vec), `list(include_deleted)`. Search: `fts_search` (FTS5 bm25, query escaped by `fts_quote`: drop stopwords, quote tokens, OR-join; limit clamped so it can never read as `LIMIT -1`). Vectors: `set_embedding` (768 dims enforced, delete-then-insert into vec0), `knn` (L2, skips soft-deleted).

- **remem-graph**: Cypher layer over graphqlite 0.8 on its own rusqlite connection to the same file (WAL + 5s busy timeout; WAL race handled by re-reading the pragma). Labels: `Memory` (node id = memory id, props mid/kind), hubs `Agent`/`Session` with namespaced ids `agent:<aid>` / `session:<sid>`. APIs: `upsert_memory/agent/session`, `attach` (memory node + hub edges `BELONGS_TO_AGENT` / `BELONGS_TO_SESSION`), `link` (both endpoints must exist, MERGE semantics, `RELATES_TO` default), `forget`, `neighbors` (memory-to-memory only) and `neighbors_detail` (labels + direction), `cypher` (JSON in, JSON rows), `central` (PageRank over Memory nodes), `shortest_path`, `stats`, `validate`.

- **remem-embed**: local BERT via candle. `Embedder` trait (`embed(&[&str]) -> Vec<Vec<f32>>`, `dims()`), `LocalEmbedder` (mean-pool over real tokens, L2 norm, max len 512), `load()` / `load_from(dir)`: CUDA if available else CPU; model dir from `REMEM_EMBED_MODEL_DIR`, default `/home/bindesh/rag/cadet-embed-base-v1`. 768 dims.

- **remem-recall**: the engine and pure ranking. `Embed` trait (minimal, local; the real embedder is adapted at the CLI/MCP edges), `StubEmbedder` (hashed n-grams, offline fallback), `RecallEngine` (Store + optional Graph + embedder + Weights + half_life_days + min_score). `rank.rs`: `rrf`, `fuse` (weighted RRF over ranked id lists, stable), `recency_score` (exponential decay, 30d half-life), `final_score` (fused * (0.5 + 0.5*importance) * (0.7 + 0.3*recency)), `pack_by_budget` (skip over-budget hits, top hit always kept), `apply_floor` (may return empty). `src/main.rs`: `remem` CLI (remember/recall/list/forget/purge/link/related/central/path/stats/validate, `--min-score`, `--since`/`--until` as unix seconds or YYYY-MM-DD via `days_from_civil`).

- **remem-mcp**: MCP stdio server, hand-rolled newline-delimited JSON-RPC 2.0 (also accepts Content-Length frames, 16 MiB cap; per-message parse errors answered with -32700 and the loop continues). Tools: `remember`, `recall` (`minScore` passthrough: reopens an engine with the floor; unfiltered calls reuse the shared engine), `list`, `link`, `forget` (soft), `purge` (hard + graph forget), `related`, `central` (PageRank top-k), `path` (shortest memory-to-memory path), `stats`, `validate`. DB path from `REMEM_DB` (default `~/.remem/remem.db`), parent dirs created 0700, db + WAL sidecars chmod 0600 after open. Same engine wiring as the CLI.

## Data flow

### remember(kind, content, ...)
1. Caller (CLI or MCP) builds a `MemoryItem` (uuid id, now timestamps, defaults).
2. `RecallEngine::remember`: embed content once (`embed_one`, 768 dims).
3. `store.insert(item)`: computes sha256 content hash (kind + normalized content); if the hash already exists (even soft-deleted), returns the existing id and the rest of the pipeline still runs for that id; else inserts the row (FTS kept in sync by the AFTER INSERT trigger).
4. `store.set_embedding(id, vec)`: delete-then-insert into `mem_vec` keyed by rowid.
5. `graph.attach(item)`: upsert Memory node, upsert Agent/Session hubs (if agent_id / session_id set), link BELONGS_TO_AGENT / BELONGS_TO_SESSION.

### recall(query, k, filters...)
1. Trim text; empty returns []. k defaults to 5; candidate depth = 4*k per list.
2. Filter pass: `store.list(false)` filtered in memory by kind/tags/agent/session/since/until (on the event clock) into an allowed id set.
3. `store.fts_search(text, depth)` -> bm25-ranked ids, intersected with allowed.
4. Embed the query; `store.knn(qvec, depth)` -> distance-ranked ids, intersected with allowed.
5. Optional graph expansion: RRF-fuse the fts + vector lists, take up to min(k, 8) seeds, collect `graph.neighbors(seed)` ids in the allowed set (one hop).
6. `fuse` the fts / vector / graph lists (weighted RRF k=60) with reasons like `fts#1`.
7. For each fused id: `store.get` (skips rows deleted mid-flight), recency on the event clock, `final_score` = fused * importance factor * recency factor; reasons gain `recent` (recency > 0.9) and `important` (importance >= 0.8).
8. Sort desc, truncate to k, `pack_by_budget` if max_chars set, `apply_floor` last (floor may empty the result; default floor 0.0 = off).

## Schema (crates/remem-store/schema.sql)

- `memories`: id TEXT PK, kind, content, tags (JSON string), agent_id, session_id, importance REAL default 0.5, created_at, updated_at, occurred_at (nullable), deleted INT default 0 (soft delete), content_hash.
- `idx_memories_content_hash`: UNIQUE index on content_hash (dedup backstop).
- `memories_fts`: FTS5 virtual table over content, external-content mode (content=`memories`, content_rowid=rowid), tokenizer `porter unicode61`.
- Triggers: `memories_ai` (insert -> FTS add), `memories_ad` (delete -> FTS remove), `memories_au` (update of content -> FTS remove + re-add).
- `mem_vec`: vec0 virtual table, embedding FLOAT[768], rowid = memories.rowid.
- Graph tables live beside these in the same file, created and owned by graphqlite (`nodes`, `edges`, `node_labels`, `node_props_text`, `property_keys`, ...), with FK ON DELETE CASCADE and per-connection `PRAGMA foreign_keys = ON`.

## Where cross-cutting concerns live

- Migrations: `Store::open` (pre-schema ALTER TABLE for old files, tolerant of "no such table"/"duplicate column") plus `migrate()` (pragma_table_info checks, idempotent index creation). No version table; migrations are add-column-only.
- Dedup: `content_hash` computed by `Store::insert`/`update` (sha256 of kind + lowercased whitespace-collapsed content); unique index enforces it.
- Fusion/recency/floor/packing: `remem-recall/src/rank.rs`, pure functions; the floor default (0.0 = off) and calibration notes live in `remem-recall/src/lib.rs` (`DEFAULT_MIN_SCORE`), applied via `RecallEngine::with_min_score` / CLI `--min-score` / MCP `minScore`.
- Scope filtering: `matches()` in `remem-recall/src/lib.rs`; allowed-set intersection happens after each ranked list so scoped-out hits never leak.
- Embedding fallback: `StubEmbedder` (remem-recall) chosen by CLI and MCP `engine()` when `remem_embed::load()` fails; real `LocalEmbedder` otherwise.
- Validation: `Graph::validate` (dangling edges + orphan memories, SQL failures become report lines, not errors); surfaced by CLI `remem validate` (exit 1 on issues) and MCP `validate`. Own-API writes cannot dangle (graphqlite FK + cascade); the check targets foreign writers and schema drift.
- Security hardening: file permissions (0700 dirs / 0600 db) in remem-mcp, FTS query escaping and limit clamping in remem-store, frame size cap in remem-mcp, model-dir trust-boundary docs in remem-embed.
- Concurrency: two connections (store + graph) on one file, both WAL with 5s busy timeout; vec0/sqlite-vec pinned =0.1.9 and registered once per process.

## Diagrams

- [component](diagrams/component.puml) ([png](diagrams/component.png)): CLI/MCP binaries -> RecallEngine -> store/graph/embed crates -> one SQLite file.
- [remember sequence](diagrams/sequence-remember.puml) ([png](diagrams/sequence-remember.png)): agent -> CLI/MCP -> embed -> store -> graph, with similar[]/dedup decisions.
- [recall sequence](diagrams/sequence-recall.puml) ([png](diagrams/sequence-recall.png)): agent -> engine -> FTS + vector + graph-hop -> RRF fuse -> pack -> floor -> hits.
- [schema ER](diagrams/schema-er.puml) ([png](diagrams/schema-er.png)): memories + FTS/vec/graph tables, rowid links, nullable fields.
- [recall activity](diagrams/activity-recall.puml) ([png](diagrams/activity-recall.png)): parse -> retrieve -> RRF -> pack -> floor, empty/over-budget branches.
