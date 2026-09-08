# ReMem v3 — crate contracts (Rust)

Workspace: `remem/Cargo.toml`. Shared data types: `remem-types` (done, do not modify).
Embedding dim is **768** everywhere. One SQLite file holds relational + FTS5 + vec0 + graph tables.

## remem-store (owner: glm worker) — `crates/remem-store/`

```toml
[dependencies]
remem-types = { path = "../remem-types" }
rusqlite = { version = "0.32", features = ["bundled"] }
sqlite-vec = "=0.1.9"
zerocopy = "0.7"
serde_json = "1"
```

- Register sqlite-vec via `sqlite3_auto_extension` (see https://alexgarcia.xyz/sqlite-vec/rust.html).
- Schema (`schema.sql`, applied on open):
  - `memories(id TEXT PK, kind TEXT, content TEXT, tags TEXT JSON array, agent_id TEXT, session_id TEXT, importance REAL, created_at INT, updated_at INT, deleted INT DEFAULT 0)`
  - `memories_fts` FTS5 on content, trigger-maintained on insert/update/delete.
  - `mem_vec(rowid INTEGER PK, embedding FLOAT[768])` vec0 virtual table keyed by memories.rowid.
- API: `Store::open(path)`, `insert(item: &MemoryItem) -> id`, `get(id) -> Option<MemoryItem>`,
  `fts_search(query, limit) -> Vec<(MemoryItem, f32 rank)>`, `set_embedding(id, &[f32])`,
  `knn(vector: &[f32], k) -> Vec<(String id, f32 distance)>` via `vec_distance_L2`.
- TDD: CRUD roundtrip, FTS finds keywords, KNN returns nearest by meaning (hand-made vectors OK).

## remem-graph (owner: qwen worker) — `crates/remem-graph/`

```toml
[dependencies]
remem-types = { path = "../remem-types" }
rusqlite = { version = "0.32", features = ["bundled"] }
graphqlite = "0.8"
serde_json = "1"
```

- Research the graphqlite 0.8 Rust API first (`cargo doc`, opensrc skill, docs at https://colliery-io.github.io/graphqlite/).
- MUST verify: graphqlite + sqlite-vec coexist on one file/connection. If not, graph opens its own connection to the same file.
- Memory nodes: label `Memory`, property `mid` = memory id, `kind`. Edges: `RELATES_TO` default, custom types allowed.
  Hub nodes: `Agent {aid}`, `Session {sid}` with `BELONGS_TO_AGENT` / `BELONGS_TO_SESSION` edges.
- API: `Graph::open(path)`, `upsert_memory(id, kind)`, `link(from_mid, to_mid, rel)`,
  `neighbors(mid) -> Vec<(String mid, String rel)>`, `cypher(q, params_json) -> serde_json::Value`.
- TDD on `:memory:`: link then neighbors, cypher MATCH roundtrip.

## remem-embed (owner: muse worker) — `crates/remem-embed/`

- Local model dir: `/home/bindesh/rag/cadet-embed-base-v1` (BERT, 768, safetensors) and
  pre-exported ONNX at `/home/bindesh/prime-agent/remem/models/onnx/model.onnx`.
  Tokenizer: same cadet dir (`tokenizer.json`).
- Pick runtime via opensrc research: `ort` (ONNX, reuse existing export) or `candle` (safetensors direct).
  Requirement: GPU first (GTX 1660 Ti, CUDA at /opt/cuda), CPU fallback that always works.
- Trait (define in this crate, recall depends on it):
  `trait Embedder { fn embed(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>>; fn dims(&self) -> usize; }`
  plus `fn load() -> anyhow::Result<impl Embedder>` using the paths above.
- Deps: `anyhow = "1"`, `remem-types` not needed. Mean-pool + L2 normalize (match v2 behavior).
- TDD: dims==768, vectors L2-normalized, cos(related) > cos(unrelated) on 3 hand sentences.
  Mark GPU tests `#[ignore]` if no GPU on machine; CPU path must pass in CI.

## remem-recall + CLI (owner: agy worker) — `crates/remem-recall/`

```toml
[dependencies]
remem-types = { path = "../remem-types" }
remem-store = { path = "../remem-store" }
remem-graph = { path = "../remem-graph" }
serde_json = "1"
clap = { version = "4", features = ["derive"] }
anyhow = "1"
```

- Port ranking from v2 TS `src/recall.ts`: RRF k=60 over FTS rank list + vector distance list,
  recency half-life 30d, importance weight 0.5+importance. Pure functions first, unit-tested with fake vectors.
- `RecallEngine { store, embed: Box<dyn Embedder-ish> }` — define your own minimal embed trait locally
  (`fn embed(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>>`) so you are NOT blocked by remem-embed;
  real wiring happens at integration.
- Binary `remem`: `remember <kind> <text...> [--tags a,b] [--agent id] [--session id]`,
  `recall <query...> [--k 5]`, `list`, `link <from> <to>`, `stats`.
- Integration tests with in-memory store + stub embedder.

## Global rules (all workers)

- TDD: write failing test first, then code. `cargo test -p <crate>` green before finishing.
- No emojis anywhere. Concise comments only where ambiguous.
- Do NOT `git commit` (root integrates). Do NOT touch other crates, TS files, or `remem-types`.
- Use the `refine` skill when you notice a reusable lesson; use opensrc/codegraph/software-engineering skills as needed.
- Reply to parent when done: what works, `cargo test` output, open questions.

## Resolved decisions (2026-09-08, root)
- sqlite-vec pinned `=0.1.9`: 0.1.10-alpha.4 does not compile (sqlite-vec.c includes missing sqlite-vec-diskann.c).
- `fts_search` returns raw bm25 (lower = better); recall uses ordering only.
- `set_embedding` on unknown id returns an error (kept).
- FTS tokenizer `porter unicode61` (kept).
- Graph hub ids namespaced `agent:`/`session:`; `neighbors()` returns Memory-labelled mids only.
- gemini/agy provider down (semaphore timeouts) — recall built on qwen instead.
