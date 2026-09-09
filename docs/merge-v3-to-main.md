# Merge v3 to main — recon (read-only, 2026-09-08)

## Relationship
- `main` (411c6d3) is the direct ancestor of `v3` (merge-base == main tip).
- `git log main..v3`: 87 commits. `git log v3..main`: empty.
- Merge is fast-forwardable; no divergence, no conflict resolution needed.

## What main has that v3 lacks
- Nothing worth keeping separately: v3 deletes zero main-tracked files.
- All main content preserved: `src/config/`, `src/integration/`, `src/modules/`,
  `src/tests/`, `ARCHITECTURE.md`, `SECURITY_AUDIT_2026.md`, `README.md` (identical).
- v3 only ADDS (TS v2 engine: `src/recall.ts`, `store.ts`, `graph.ts`, `embedder.ts`,
  `src/cli/`; Rust workspace: `crates/`, `Cargo.*`; `skills/`, `models/`, new docs).

## Top-level collisions
- `package.json`: full rewrite (name `remem-unified@1.0.0` -> `remem@2.0.0`;
  lancedb/better-sqlite3/drizzle/ai-sdk -> latticedb/transformers/onnxruntime; new scripts).
- `package-lock.json`: ~6800-line churn from the dep swap. Regenerate, do not hand-merge.
- `tsconfig.json`: simplified (75 lines changed). `src/index.ts`: 125 lines rewritten (v2 wiring).
- `scripts/migrate_v2.ts`, `scripts/migrate_v3.ts`: modified in place.
- No collision: `README.md`, `ARCHITECTURE.md`, audit docs unchanged; `Cargo.toml`,
  `README-v3.md`, `CHANGELOG-v3.md`, `AGENTS.md`, `skills/`, `crates/` are pure additions.

## Would a merge clobber v1/v2 history or files?
- No. Fast-forward keeps full history; v1/v2 files remain on disk and in log.
- Only semantic overwrite is the intended engine swap via the 4 files above.

## Recommended strategy: merge commit (`--no-ff`), not squash
- Rationale (1): history is linear and clean; squash would flatten 87 bisectable commits.
- Rationale (2): `--no-ff` marks the release boundary; plain fast-forward is also safe.
- Rationale (3): keep `v3` branch pointer until CI on main is green, then tag `v3.0`.
