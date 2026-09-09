# Dogfood digest — what remem remembers about itself (26 memories)

Source: `REMEM_DB=/home/bindesh/.remem/remem.db`, debug binary as-is. `remem list` (26) + `remem stats` (26 memories, 30 graph nodes, 32 edges).

## Grouped memories (26 → 7 lines)

- Security / store integrity (5 facts): unsafe transmute + mmap trust boundary; row_to_item coerces bad kind/tags silently; get/find_by_hash swallow errors to None; MCP stdio dies on one bad frame + uncapped Content-Length OOM; forget is soft-delete only (FTS + vec rows survive).
- CLI/MCP surface + parity (4 facts): 11 MCP tools vs CLI missing forget/related/central/path; --db flag vs REMEM_DB-only; k-clamp/min-score/list-limit/output-contract asymmetries; file perms dirs 0700, db+WAL 0600.
- Latency / threading (3 facts): persistent MCP saves ~0.3s/40 ops (embed ~1.5s/op dominates, model load ~0.2s once); 1.52 vs 1.53 s/op bench; Rayon all-cores optimal (10.9s→1.7s), tune via RAYON_NUM_THREADS only.
- Recall pipeline + floor (3 facts + 1 decision + 1 mistake): scope-filter → FTS+KNN@4k → 1-hop graph (≤8 seeds) → RRF k=60 → importance/recency multipliers (0.9+0.1x bands); floor off by default, evals floored at 0.02 (junk scores 0.012–0.016); multipliers still outvote dual rank-1 signals (fused 31/37 < vector 34/37).
- Store ops (2 facts + 1 mistake): online backup via sqlite .backup/VACUUM INTO, restore = file copy writers-stopped, .db only; content-hash (sha256, normalized) UNIQUE dedup returns existing id; legacy-DB open needs ALTERs-before-schema + regression test.
- Concurrency / migration rules (2 facts): WAL + 5s busy_timeout on every connection (20/20 vs 1/19 BUSY), short write txns, one process; add-column-only idempotent migrations via pragma_table_info, no version table.
- Workflow / providers / arch (1 decision + 1 mistake + 3 facts): v3 = rusqlite+sqlite-vec 0.1.9+graphqlite 0.8, candle, RRF, clap; never full-rewrite another agent's test file; nemotron-3-ultra fallback OK / opencode+deepseek 401-dead; gemini-saturated→qwen; muse selector is opencode/muse-spark-1.3-contributor-free.

Kind counts: fact 20, mistake 4, decision 2.

## Coverage gaps (v3 subsystems with NO memory)

- remem-types: nothing on MemoryKind/Item schema or additive-only rule.
- remem-graph/graphqlite: nothing on hub ids (agent:/session:), neighbors()-returns-mids-only, Cypher alias vs SQL keyword trap, FK/cascade behavior.
- remem-embed model loading: nothing on candle BERT, model dir / integrity check, --features cuda vs CPU fallback, sqlite-vec 0.1.9 pin reason.
- Graph recall tools: nothing on related/central/path/link/validate behavior or when to use them.
- Tag/importance/recency tuning: nothing on tagboost, importance bands, or scope-filter semantics beyond the pipeline line.
- Ops runbook: backup rule exists, but nothing on concurrency-recheck, fuzz/mcp-fuzz findings, eval harness usage, or release checklist.

## 5 candidate memories to store next (ready to paste)

1. kind=fact — "remem-graph hub convention: node ids namespaced agent:/session:, neighbors() returns Memory mids only; use Cypher MATCH for string-property lookups (findNodesByLabelProperty fails on strings); never txn.commit() inside db.write()." (covers graph gap)
2. kind=fact — "sqlite-vec pin: remem-store requires sqlite-vec =0.1.9; 0.1.10-alpha.4 fails to compile (missing sqlite-vec-diskann.c); validate version in the real workspace, not an isolated copy." (covers embed/store gap)
3. kind=fact — "remem-embed model trust: candle BERT (cadet-embed-base-v1) mmaps model.safetensors from REMEM_EMBED_MODEL_DIR with no integrity check; treat model dir as trusted, document and verify before use; GPU only via --features cuda, CPU default." (covers trust gap)
4. kind=decision — "Dogfood write rule: agents save non-obvious fixes (mistake), design rationale (decision), env facts (fact) with tags+agent; recall before touching unfamiliar code; floor/eval numbers always state floored vs unfloored." (covers protocol gap)
5. kind=mistake — "MCP recall gap (tracker 3): MCP recall lacks since/until while CLI has them; MCP clamps k/limit to 1000 while CLI does not — check the clamp before paginating large recalls." (covers parity gap)
