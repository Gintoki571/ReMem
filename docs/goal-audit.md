# Goal audit — v3 thread goal (2026-09-09, branch v3)

Thread goal: Rust long-term memory (SQLite+GraphQlite+sqlite-vec, local embeddings GPU+CPU fallback) with TDD, CI/CD, GitHub push, live dogfooding.

| # | Requirement | Verdict | Evidence |
|---|-------------|---------|----------|
| 1 | Rust system | MET | 6-crate workspace: remem-types/store/graph/embed/recall/mcp; CLI `remem-recall/src/main.rs`, MCP server `crates/remem-mcp` |
| 2 | One-file SQLite stack | MET | `remem-graph/src/lib.rs` (graphqlite 0.8, shared connection); `remem-store/src/lib.rs` (sqlite-vec 0.1.9, vec0); `coexistence.rs` 3/3 green |
| 3 | Embeddings + fallback | MET | `remem-embed/src/lib.rs` `Device::cuda_if_available(0).unwrap_or(Device::Cpu)`; `cuda` cargo feature; live `stats`: `embedder: Cpu (768d)` |
| 4 | TDD totals | MET | `TMPDIR=$PWD/target/audit-tmp cargo test --workspace -- --test-threads=4` (2026-09-09): 20 suites, 159 passed, 0 failed, 2 ignored (CUDA-only + bench). Default TMPDIR fails on host (/tmp WAL quota + parallel MERGE flake) — environmental, not code |
| 5 | CI state | PARTIAL | `.github/workflows/ci.yml` jobs test/rust/demo. Latest runs 34308488286 + 34308362414: `test` + `demo` green, `rust` red on `cargo fmt --check` only (`rank.rs:430` long vec line) |
| 6 | GitHub push | MET | Branch `v3`, remote `origin https://github.com/Gintoki571/ReMem.git`; pushes trigger CI. Open issues: #5 GPU build blocked upstream (candle-kernels vs CUDA 13.3 sm_75), #6 tag-anchor ranking follow-up (Q27/Q28 supervision) |
| 7 | Dogfooding | MET | Live `REMEM_DB=~/.remem/remem.db` via `stats`: 32 memories, 37 nodes, 39 edges; `skills/remem/SKILL.md` SYNCED with installed skill; MCP `settings.json` (`mcpServers.remem`), 5 tools (remember/recall/link/forget/stats) |

## Verdicts

Rust MET, SQLite stack MET, embeddings+fallback MET, TDD MET, CI PARTIAL, push MET, dogfooding MET.

## Top remaining item

Run `cargo fmt` (`rank.rs:430`) so `rust` job goes green, then merge v3 to main per `docs/landing-checklist.md`. Next: #6 tokenizer gap (Q28 unchanged by design, adv battery 35/37); #5 blocked upstream.
