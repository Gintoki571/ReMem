# DOX framework

- DOX is highly performant AGENTS.md hierarchy installed here
- Agent must follow DOX instructions across any edits

## Core Contract

- AGENTS.md files are binding work contracts for their subtrees
- Work products, source materials, instructions, records, assets, and durable docs must stay understandable from the nearest applicable AGENTS.md plus every parent AGENTS.md above it

## Read Before Editing

1. Read the root AGENTS.md
2. Identify every file or folder you expect to touch
3. Walk from the repository root to each target path
4. Read every AGENTS.md found along each route
5. If a parent AGENTS.md lists a child AGENTS.md whose scope contains the path, read that child and continue from there
6. Use the nearest AGENTS.md as the local contract and parent docs for repo-wide rules
7. If docs conflict, the closer doc controls local work details, but no child doc may weaken DOX

Do not rely on memory. Re-read the applicable DOX chain in the current session before editing.

## Update After Editing

Every meaningful change requires a DOX pass before the task is done.

Update the closest owning AGENTS.md when a change affects:

- purpose, scope, ownership, or responsibilities
- durable structure, contracts, workflows, or operating rules
- required inputs, outputs, permissions, constraints, side effects, or artifacts
- user preferences about behavior, communication, process, organization, or quality
- AGENTS.md creation, deletion, move, rename, or index contents

Update parent docs when parent-level structure, ownership, workflow, or child index changes. Update child docs when parent changes alter local rules. Remove stale or contradictory text immediately. Small edits that do not change behavior or contracts may leave docs unchanged, but the DOX pass still must happen.

## Hierarchy

- Root AGENTS.md is the DOX rail: project-wide instructions, global preferences, durable workflow rules, and the top-level Child DOX Index
- Child AGENTS.md files own domain-specific instructions and their own Child DOX Index
- Each parent explains what its direct children cover and what stays owned by the parent
- The closer a doc is to the work, the more specific and practical it must be

## Child Doc Shape

- Create a child AGENTS.md when a folder becomes a durable boundary with its own purpose, rules, responsibilities, workflow, materials, or quality standards
- Work Guidance must reflect the current standards of the project or user instructions; if there are no specific standards or instructions yet, leave it empty
- Verification must reflect an existing check; if no verification framework exists yet, leave it empty and update it when one exists

Default section order:
- Purpose
- Ownership
- Local Contracts
- Work Guidance
- Verification
- Child DOX Index

## Style

- Keep docs concise, current, and operational
- Document stable contracts, not diary entries
- Put broad rules in parent docs and concrete details in child docs
- Prefer direct bullets with explicit names
- Do not duplicate rules across many files unless each scope needs a local version
- Delete stale notes instead of explaining history
- Trim obvious statements, repeated rules, misplaced detail, and warnings for risks that no longer exist

## Closeout

1. Re-check changed paths against the DOX chain
2. Update nearest owning docs and any affected parents or children
3. Refresh every affected Child DOX Index
4. Remove stale or contradictory text
5. Run existing verification when relevant
6. Report any docs intentionally left unchanged and why

## User Preferences

When the user requests a durable behavior change, record it here or in the relevant child AGENTS.md

## Child DOX Index

- No child AGENTS.md yet. `src/` modules are owned by this file (see Layout).

# neocortex AGENTS.md

## Purpose

Long-term memory for AI agents, v0.1.4. Rust, one crate, one binary `neocortex` (CLI + MCP stdio server). One SQLite file holds memories (FTS5 BM25 + sqlite-vec), an entity/relation knowledge graph (graphqlite projection) and bi-temporal facts. Embeddings: UltraBERT-Embed-150M on candle (pure Rust, CPU). No LLM inside, no Python, no network.

Design (binding): `docs/superpowers/specs/2026-10-22-neocortex-design.md`. Plan: `docs/superpowers/plans/2026-10-23-neocortex-plan.md`. Pre-rewrite ReMem code: git tag `remem-final`.

## Ownership

- Orchestrator/validator: Claude (merges lanes, runs gates, owns this file and the design doc).
- Lanes (external harness agents) own only the files named in their brief, on branch `lane-<name>` in a git worktree under `../neocortex-lanes/<name>`.

## Local Contracts

- Version stays `0.1.4` until the user says otherwise.
- Dependencies: rusqlite (bundled), sqlite-vec `=0.1.9` (0.1.10-alpha does not compile), graphqlite `0.12`, candle-core/candle-nn `0.11`, tokenizers (`fancy-regex`, no default features), serde, serde_json. Adding any other crate needs a one-line reason here.
- Relational tables are the source of truth; graphqlite nodes/edges are a projection rebuilt by `neocortex reindex`.
- One connection per process (`db::Db`): sqlite-vec via `sqlite3_auto_extension`, graphqlite via `Connection::from_rusqlite`. Writes go through `Db::write` (BEGIN IMMEDIATE). Cypher runs inside the same transaction.
- Validity is half-open `[valid_from, valid_to)`; system retirement is `expired_at`. Times are unix seconds UTC.
- One live memory per `content_hash` (unique partial index); a hash hit only counts as a duplicate when kind + folded text match. A memory's end (`--until`, `invalidate m`, supersede) ends the relations it alone still evidenced; soft forget retracts them (`expired_at`, no end date); hard forget and merge patch the graph projection incrementally, never `rebuild_all`.
- Embedding-only similarity never merges entities; it only adds `similar` soft edges.
- Ids shown to agents: `m<id>`, `e<id>`, `r<id>`; tools accept both forms.
- `recall` adds a query-directed path channel (`paths::search`, beam 24, depth 3, `PATH_WEIGHT` 2.5) plus a semantic multi-hop channel (`paths::search_semantic`: relation KNN k=100, residual-question hop scoring, >= 2-hop chains, weight `w_sempath` 0.25) and renders the path facts; it is skipped on negation and history-intent questions, and an empty recall is returned when no lexical, entity, relation or path support exists and the best cosine is below `ABSTAIN_COSINE` (0.18).
- The embedder sets `RAYON_NUM_THREADS` to candle's thread count at model load when unset (gemm and rayon pools must agree; 2x faster).
- Env: `NEOCORTEX_EMBEDDER=hash` (tests only; a DB records its embedder and refuses another), `NEOCORTEX_DB` (default `~/.neocortex/neocortex.db`), `NEOCORTEX_MODEL_DIR` (default `~/.neocortex/models/UltraBERT-Embed-150M`), `NEOCORTEX_TZ` (`+HH:MM`, default system local offset), `NEOCORTEX_MAX_TOKENS` (default 1024), `NEOCORTEX_EXTRACTOR` (unset = off; `rules` = `src/extract_model.rs` relation/date tier, used only when a memory has no agent-supplied entities or relations).

## Layout

- `src/db.rs` + `src/schema.sql` - connection, extensions, schema. The schema version lives in the `schema_meta` table (v3), never in `PRAGMA user_version`: graphqlite owns that pragma (it stamps 1 and its cypher executor refuses any other value). Migrations are an explicit ladder in `Db::init`.
- `src/embed.rs`, `src/ultrabert.rs` - Embedder trait, UltraBERT forward pass, HashEmbedder (tests).
- `src/text.rs` - normalisation, singularise, Jaro-Winkler, trigrams, FTS query building, predicate keys, content hash.
- `src/temporal.rs` - civil dates, instant/window parsing, query windows, history intent.
- `src/extract.rs` - heuristic entity candidates, n-grams for alias lookup.
- `src/extract_model.rs` - optional deterministic relation/date tier (off by default). Scored by `examples/extract_eval.rs` on `eval/extract/gold.json` (128 memories; `cargo run --release --example extract_eval -- [gold] [predictions.json] [name]`). Reference predictions and runner scripts: `eval/extract/preds/`, `eval/extract/reference/`. Comparison and candle port plan: `docs/research/extraction.md`.
- `src/graph.rs` - CSR + ACL push personalized PageRank. `src/rank.rs` - RRF, MMR, cosine, packing.
- `src/remember.rs`, `src/resolve.rs`, `src/kg.rs` - write pipeline, entity/predicate resolution, and rebuildable graph projection. `remember` and `remember_batch` share `prepare` + `write_prepared`; a batch chunk of <= 256 inputs is one embed call and one transaction with a SAVEPOINT per item.
- `src/recall.rs` - ranked multi-channel memory, fact, and entity retrieval; `src/eval.rs` - fixture-based retrieval evaluation and ablations.
- `src/paths.rs` - LLM-free query-directed path search over relations (an RRF channel in recall), plus the agent tools `follow` (bounded traversal, `reached` flag) and `aggregate` (grouped counts with a `negative` bucket). Also owns negation cues (`has_negation_cue`, `negation_intent`).
- `tests/fixtures/ultrabert_fixture.json` - authoritative embedding reference (sentence-transformers 6.2.0, fp32).
- `src/engine.rs` - public API (Engine, input/output types = MCP tool schema), Config tunables.
- `src/mcp.rs` - MCP stdio server (JSON-RPC 2.0, 10 tools, LazyEmbedder). `src/render.rs` - compact text output. `src/main.rs` - CLI. `src/import.rs` - one-shot ReMem v3 import.
- `src/export.rs` - JSON Lines `export`/`import` (backup and embedder migration: re-ingest through remember). `src/bench.rs` - synthetic corpus + latency/size report (`neocortex bench --n N`). `src/eval.rs` - `neocortex eval` (ablation, failures, `--set`, `--sweep` on one ingestion).
- `tests/temporal_fixes.rs` - regression tests for the temporal/invalidation hunt (until/invalidate/forget ends, interval overlap, `--when` spans, text-date inference, budget and duplicate flags).
- `tests/ops.rs` - export/import round trip, concurrent writer processes. `tests/world.rs` - batch == sequential on a mixed workload (ids, whole outputs, tables) including a negation, a seeded single-valued change and a non-exclusive conflict, batch error isolation, and the committed world fixture's key/answer integrity.
- `eval/fixtures.json` (tuning, 95 memories / 84 queries) + `eval/holdout.json` (independent check, 105 / 88) + `eval/world-300.json` (generated, 300 people / 3264 memories / 600 queries) + `eval/README.md` (format, scoring incl. acc@10, tuning protocol: defaults change only if both sets agree, world protocol).
- `eval/extract/holdout.json` - independent held-out extraction set (150 memories, `content` + `entities` + `relations` + `valid_from`), scored by `examples/extract_eval.rs`; never tuned against, never read by extraction code.
- `examples/gen_world.rs` - deterministic (`--seed`) synthetic year of agent memory + ground-truth queries; the only writer of `eval/world-*.json`. Generated worlds over 2 MB stay out of git. `examples/agent_sim.rs` - iterative agent simulation (recall -> follow -> recall-leaf) over a world fixture and an ingested DB, scoring exactly like eval acc@10.
- `docs/research/multihop.md` - measured multi-hop/aggregation/negation/abstention results: path-channel ablations, fixtures/holdout gate, latency, agent simulation, recommendation.
- `skills/neocortex/SKILL.md` - agent-facing usage protocol; `README.md` - user docs.

## Work Guidance

- TDD: failing test first, then the smallest fix (skills: test-driven-development, systematic-debugging; ponytail style: smallest complete change, no speculative abstraction).
- Write from the agent's perspective: compact outputs, short ids, forgiving inputs, errors that say what to do.
- Lanes never commit outside their branch, never push, never edit files outside their brief.

## Verification

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test            # model-dependent tests skip when the model dir is absent
cargo build --release && ./target/release/neocortex eval eval/fixtures.json --ablation   # gate: fixtures recall >= 0.95 / MRR >= 0.89; holdout (eval/holdout.json) recall >= 0.92 / MRR >= 0.78
NEOCORTEX_EMBEDDER=hash ./target/release/neocortex bench --n 10000 --db /tmp/b.db      # recall p50 <= 150 ms
cargo run --release --example gen_world -- --people 300 --seed 7 --out eval/world-300.json   # must match the committed file byte for byte
./target/release/neocortex eval eval/world-300.json --save-db /tmp/world300.db              # per-category acc@10 / recall / MRR
cargo run --release --example agent_sim -- eval/world-300.json target/w300.db              # iterative agent accuracy (recall -> follow -> recall-leaf)
```
