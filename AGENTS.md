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

This project is not yet indexed. Before continuing you must scan the project, build the DOX tree and replace this message with the actual index. Go deep and scan files recursively to properly evaluate complexity and create nested DOX files where needed.


# Remem AGENTS.md

Local-first long-term memory engine for AI agents. Rust (v3) is main since
a9d18f6 retired the TypeScript tree; v2 is retired and kept only as
README-v2.md.

Docs: `docs/architecture-v3.md` (design), `docs/ops.md` (build/run). Read both before changing code.

### Skills
- Memory protocol: `skills/remem/SKILL.md` — dogfood remember/recall.
- Worker contract: `skills/tdd-worker/SKILL.md` — failing test first, then fix.

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

Local gate: `.git/hooks/pre-commit` runs `cargo fmt --check` (not tracked by git; reinstall if hooks dir reset).
CI enforces fmt + clippy + test on every push.
Always verify all three green before pushing.

TDD: add/update the failing test first, then the fix; never commit a red workspace.

### Layout (6 crates)

- `crates/remem-types` - shared domain types (MemoryKind/Item, RecallQuery/Hit). Additive changes only.
- `crates/remem-store` - rusqlite bundled + sqlite-vec **=0.1.9** (0.1.10-alpha.4 does not compile: missing sqlite-vec-diskann.c). Schema in `schema.sql`. Also records corroboration merges (issue #14) in the `merges` table (FTS aux `merges_fts` over absorbed content).
- `crates/remem-graph` - graphqlite 0.8 over the same file. Hub ids `agent:`/`session:` namespaced; `neighbors()` returns Memory mids only.
- `crates/remem-embed` - candle BERT (cadet-embed-base-v1), GPU via `--features cuda`, CPU fallback default. Tests skip when model dir absent (CI).
- `crates/remem-recall` - RRF fusion + `remem` binary. Real embedder wired in `main.rs` with stub fallback.
- `crates/remem-mcp` - MCP server (stdio) over the same engine. 11 tools: remember, recall, list, link, forget, purge, stats, validate, related, central, path.

### CLI (`./target/debug/remem`, global `--db`, `$REMEM_DB`)

- `remember <kind> <text> [--tags t1,t2] [--agent a] [--session s] [--importance f] [--occurred-at secs|YYYY-MM-DD]`
- `recall <query> [--k n] [--json] [--agent a] [--session s] [--since x] [--until y] [--max-chars n] [--min-score f] [--no-recency] [--half-life-days d]`
- `graph --html <file>` — export the whole knowledge graph as a self-contained HTML file (canvas spring layout, click node for content, kind filter; no CDN/server; data via remem-graph `Graph::edges()`)
- `list [--json]` | `link <from> <to> [--rel r] [--provenance manual|recall-suggested|correction-chain|auto-link]` (default manual) | `correct <old-id> <kind> <text>` (supersede + correction-chain SUPERSEDES edge) | `forget <id>` | `purge <id>` | `stats` | `validate` (exit 1 on health: dangling/unreviewed/suspect-SUPERSEDES/ghost/missing-node; orphans via `--show-orphans`, exit 0) | `related <id>` (id/rel/provenance) | `central` (id/score/provenance-summary) | `path <from> <to>` | `trace <id>` (correction chain, oldest->newest, then `merged: <absorbed content>` lines; empty if unknown)` | `supersede <old-id> <kind> <text...>` (soft-supersede + replacement row with `supersedes` back-pointer; prints the new id; errors on unknown/deleted/already-superseded)
- Edge provenance: missing prop reads as manual (old DBs validate clean); graph-fused recall hits carry `prov:<value>` reasons.
- Write-time auto-link: `remember` runs vec0 kNN (top-3) post-insert; exactly ONE candidate at cosine >= 0.72 writes an `auto-link` edge, 2-3 park ALL in `suggested_edges` (query `(auto-link)`), 0 does nothing. Auto-link edges are excluded from recall graph expansion; validate reports them as unreviewed (machine-written).

### Tests per crate

```bash
cargo test -p remem-types -p remem-store -p remem-graph -p remem-recall -p remem-mcp
cargo test -p remem-embed                  # skips without model dir
cargo test -p remem-embed --features cuda  # GPU path (needs CUDA 12.x; broken on 13.x, see docs/cuda-unblock.md)
```

### Agent memory protocol (dogfood v3)

```bash
export REMEM_DB="$HOME/.remem/remem.db"
./target/debug/remem remember <kind> "<content>" --tags t1,t2 --agent <name> --importance 0.8
./target/debug/remem recall "<query>" --k 5 [--agent x] [--session y]
```
