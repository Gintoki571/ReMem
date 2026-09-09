# Merge v3 into main — PR draft

Title: Merge v3 (Rust memory engine) into main

## What
Merge branch `v3` into `main` (fast-forwardable, `--no-ff` to mark release boundary).
Adds 6-crate Rust workspace (types/store/graph/embed/recall/mcp + `remem` CLI),
one-file SQLite stack (FTS5 + sqlite-vec 0.1.9 + graphqlite 0.8 Cypher),
local CPU embeddings, RRF recall (k=30) with floor-first + gated tag boost,
11-tool CLI/MCP parity, skill + docs (41 files).

## Why
v3 meets all 9 goal-audit items: fused recall criterion MET, CI green,
dogfooded live DB, issues #1-4 closed. main is direct ancestor (no conflicts).

## Numbers
- Tests: 163 passed, 0 failed, 2 ignored, 21 suites (`cargo test --workspace`).
- Recall: fused 35/37 @1 >= best single 34/37; @5 35/37 (floor drops Q27 by design).
- Adversarial: 3/3 pass (floor-before-boost ordering, min-score 0.017).
- MCP: 11 tools at `tools/list`; live DB 34+ memories; skill ~2849 chars.
- Closes: #1 (floor default), #2 (near-dup probe), #3 (CLI forget/related/central/path), #4 (MCP occurredAt).

## Known gaps (open, not blockers)
- #5: GPU build blocked upstream (candle-kernels vs CUDA 13.3 on sm_75); CPU supported.
- #6: tag-anchor supervision follow-up (Q27/Q28 need real signal beyond 1.05x/2.0x gate).

## Reviewers checklist
- [ ] CI green on merge commit (test + rust fmt/clippy + demo jobs).
- [ ] `cargo test --workspace` reproduces 163/0/2 locally.
- [ ] `REMEM_DB=/tmp/remem-demo.db scripts/demo.sh` exits 0.
- [ ] `scripts/eval.sh` reproduces 35/37 @1 and 3/3 adversarial (or accepted 2/3 per #6).
- [ ] Skill sync: `skills/remem/SKILL.md` matches 11 MCP tools + floor behavior.
- [ ] Keep `v3` branch pointer until main CI is green, then tag `v3.0`.
