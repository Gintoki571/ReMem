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
- Embedding-only similarity never merges entities; it only adds `similar` soft edges.
- Ids shown to agents: `m<id>`, `e<id>`, `r<id>`; tools accept both forms.
- Env: `NEOCORTEX_EMBEDDER=hash` (tests only; a DB records its embedder and refuses another), `NEOCORTEX_DB` (default `~/.neocortex/neocortex.db`), `NEOCORTEX_MODEL_DIR` (default `~/.neocortex/models/UltraBERT-Embed-150M`), `NEOCORTEX_TZ` (`+HH:MM`, default UTC), `NEOCORTEX_MAX_TOKENS` (default 1024).

## Layout

- `src/db.rs` + `src/schema.sql` - connection, extensions, schema (PRAGMA user_version).
- `src/embed.rs`, `src/ultrabert.rs` - Embedder trait, UltraBERT forward pass, HashEmbedder (tests).
- `src/text.rs` - normalisation, singularise, Jaro-Winkler, trigrams, FTS query building, predicate keys, content hash.
- `src/temporal.rs` - civil dates, instant/window parsing, query windows, history intent.
- `src/extract.rs` - heuristic entity candidates, n-grams for alias lookup.
- `src/graph.rs` - CSR + ACL push personalized PageRank. `src/rank.rs` - RRF, MMR, cosine, packing.
- `src/remember.rs`, `src/resolve.rs`, `src/kg.rs` - write pipeline, entity/predicate resolution, and rebuildable graph projection.
- `tests/fixtures/ultrabert_fixture.json` - authoritative embedding reference (sentence-transformers 6.2.0, fp32).
- `src/engine.rs` - public API (Engine, input/output types = MCP tool schema), Config tunables.
- `src/mcp.rs` - MCP stdio server (JSON-RPC 2.0, 8 tools, LazyEmbedder). `src/render.rs` - compact text output. `src/main.rs` - CLI. `src/import.rs` - one-shot ReMem v3 import.
- `eval/fixtures.json` + `eval/README.md` - retrieval benchmark (95 memories, 84 queries, 11 categories) and scoring rules.
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
```
