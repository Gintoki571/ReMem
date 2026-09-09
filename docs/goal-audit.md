# Goal audit — v3 thread goal (2026-09-09, branch v3)

Thread goal: Rust long-term memory (SQLite+GraphQlite+sqlite-vec, local embeddings GPU+CPU fallback) with TDD, CI/CD, GitHub push, live dogfooding.

| # | Requirement | Verdict | Evidence |
|---|-------------|---------|----------|
| 1 | Rust system | MET | 6-crate workspace: remem-types/store/graph/embed/recall/mcp; CLI `crates/remem-recall/src/main.rs`, MCP server `crates/remem-mcp` |
| 2 | One-file SQLite stack | MET | `remem-graph/src/lib.rs` (graphqlite 0.8, `Graph::from_connection` shares store connection); `remem-store/src/lib.rs` (sqlite-vec 0.1.9, vec0); `coexistence.rs` 3/3 green |
| 3 | Embeddings + fallback | MET | `remem-embed/src/lib.rs:119` `Device::cuda_if_available(0).unwrap_or(Device::Cpu)`; `cuda` cargo feature; live `remem stats`: `embedder: Cpu (768d)` |
| 4 | TDD totals | MET | `TMPDIR=$PWD/target/audit-tmp cargo test --workspace -- --test-threads=4` (2026-09-09): 20 suites, 155 passed, 0 failed, 2 ignored (CUDA-only + bench). Default TMPDIR fails on host (/tmp WAL writes rejected, disk-quota poison cascade) — environmental, not code |
| 5 | CI state | PARTIAL | `.github/workflows/ci.yml` jobs test/rust/demo. Latest 2 runs red on `rust` job only: 34299452152 + 34299143844, both `cargo fmt --check` diff at `crates/remem-recall/tests/engine.rs:729` — sibling WIP, not this tree. `test` + `demo` jobs green |
| 6 | GitHub push | MET | Branch `v3`, remote `origin https://github.com/Gintoki571/ReMem.git`; pushes trigger CI (runs above ran on push). Open issues: #5 GPU build blocked upstream (candle-kernels vs CUDA 13.3 sm_75), #6 tag-anchor ranking follow-up |
| 7 | Dogfooding | MET | Live `REMEM_DB=~/.remem/remem.db` via `remem stats`: 32 memories, 37 nodes, 39 edges; skill `remem/SKILL.md`; MCP registered in `settings.json` (`mcpServers.remem`); `docs/prime-agent-integration.md` |

## Verdicts

Rust MET, SQLite stack MET, embeddings+fallback MET, TDD MET, CI PARTIAL, push MET, dogfooding MET.

## Top remaining item

Fix fmt at `engine.rs:729` (sibling WIP) so `rust` job goes green, then merge v3 to main per `docs/landing-checklist.md`.
