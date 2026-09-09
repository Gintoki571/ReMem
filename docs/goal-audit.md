# Goal audit — v3 thread goal (2026-09-09, branch v3 @ e83bf71)

Thread goal: Rust long-term memory (SQLite+GraphQlite+sqlite-vec, local embeddings GPU+CPU fallback) with TDD, CI/CD, GitHub push, live dogfooding.

| # | Requirement | Verdict | Evidence |
|---|-------------|---------|----------|
| 1 | Rust system | MET | 6-crate workspace: remem-types/store/graph/embed/recall/mcp; CLI `remem-recall/src/main.rs`, MCP server `crates/remem-mcp` |
| 2 | One-file SQLite stack | MET | `remem-graph/src/lib.rs` (graphqlite 0.8, shared connection); `remem-store/src/lib.rs` (sqlite-vec 0.1.9, vec0); `coexistence.rs` 3/3 green |
| 3 | Embeddings + fallback | MET | `remem-embed/src/lib.rs` `Device::cuda_if_available(0).unwrap_or(Device::Cpu)`; `cuda` cargo feature; live `stats`: `embedder: Cpu (768d)` |
| 4 | TDD totals | MET | `cargo test --workspace` re-verified 2026-09-09: 163 passed, 0 failed, 2 ignored (CUDA-only + bench) |
| 5 | CI state | MET | Latest v3 run 34326707256 success (`test` + `rust` + `demo`); prior 4 doc commits all success |
| 6 | GitHub push | MET | Branch `v3`, remote `origin https://github.com/Gintoki571/ReMem.git`. #1-4 CLOSED (#1 floor, #2 near-dup 0.48, #3 CLI parity, #4 MCP occurredAt); OPEN: #5 GPU upstream (candle-kernels vs CUDA 13.3 sm_75), #6 tag-anchor ranking |
| 7 | Dogfooding | MET | Live `REMEM_DB=~/.remem/remem.db` via `stats`: 47 memories, 52 nodes, 62 edges; `skills/remem/SKILL.md` SYNCED (diff clean, 2849 chars); `settings.json` (`mcpServers.remem`), 11 tools (remember/recall/list/link/forget/purge/related/central/path/stats/validate) |
| 8 | Fused recall | MET | Gated tag-boost per `docs/eval.md`: floored recall@1 35/37 (95%) >= best-single 34/37, @5 35/37 — criterion MET (Q27 floor-dropped by design) |
| 9 | Remember pipeline | MET | Floor-first ordering + tags-in-FTS landed (v3); junk gated below floor, tag anchors retrievable via FTS |
| 10 | Stemmer + hybrid (#6 path) | PARTIAL | `docs/stemmer-analysis.md` (`b62f02f` docs-only): porter divergences mapped (auditor/audit=Q28, decid/decis=Q27, +4 latent); drop-porter (32/37) and trigram (31/37, Q39 junk) rejected; hybrid (keep porter + query-side 4-char prefix arm, len>=5) IN FLIGHT, store sibling owns; predicts 35/37 @1, 37/37 @5, adv 3/3 |
| 11 | Janitor disk | OPEN | 1.4G reclaim sibling-reported, not re-measured here; live db small (3.3M db + 4.0M wal); store sibling owns |

## Verdicts

Rust MET, SQLite stack MET, embeddings+fallback MET, TDD MET (163/0/2, re-verified 2026-09-09), CI MET (34326707256 success), push MET (#1-4 CLOSED, #5/#6 OPEN), dogfooding MET (47/52/62), fused recall MET (35/37 >= 34/37), remember pipeline MET, stemmer PARTIAL (analysis done, hybrid in flight), janitor OPEN (sibling-owned).

## Top remaining item

Merge decision: v3 is fully green (CI all-success, 163/0 tests, criterion MET) — merge v3 to main per `docs/landing-checklist.md`, then land hybrid (37/37 @5 path) under #6. #5 stays blocked upstream; janitor stays with store sibling.
