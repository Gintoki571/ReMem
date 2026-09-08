# remem v3.0 release notes — DRAFT

Status: DRAFT. Branch `v3`, unreleased. Docs only, no API freeze claimed.

## Highlights

- Single-file SQLite memory engine: tables + FTS5 + sqlite-vec + graph projection.
- RRF recall fusing vector + FTS with recency/importance weighting and tag boost.
- Content-hash dedup on write; hard purge removes row, FTS entry, and embedding.
- Token-budget packing (`--max-chars`, top hit never dropped).
- Graph links with `graph#` recall reasons; `related`/`central`/`path` ops.
- `validate` health check (empty output means healthy); `stats` JSON counts.
- MCP server over stdio: 11 tools (remember/recall/list/link/forget/purge/stats/validate/related/central/path).
- 11 CLI subcommands on `remem` binary: `remember`/`recall`/`list`/`link`/`forget`/`purge`/`stats`/`validate`/`related`/`central`/`path` (full CLI/MCP parity); `list` takes `--limit`, recall `k=0` returns empty.
- Onboarding demo (`scripts/demo.sh`) and eval harness (`scripts/eval.sh`).
- CPU-by-default local embeddings (768d); CUDA opt-in via feature flag.

## Quality numbers

- Eval (40 fixtures, 37 answerable + 3 adversarial; `docs/eval.md`, HEAD `7147dc3`): unfloored
  31/37 @1 (84%), 37/37 @5; floored (`--min-score 0.02`) 31/37 @1, 35/37 @5, adversarial 3/3
  suppressed. Fused still trails vector-alone 34/37 @1; the post-RRF multipliers are the loser.
- Floored run detail: the floor costs exactly the two rank-5 tag-anchor targets Q27/Q28
  (scores below 0.02); adversarial junk (top score ~0.012-0.016) is fully suppressed at 0.02.
- Baseline (25 fixtures): 4/25 @1 (16%), 16/25 @5 (64%).
- `cargo test --workspace` (this machine, HEAD `aed76fb`): 144 passed, 0 failed, 2 ignored
  (embed `#[ignore]`d CUDA smoke + bench). The old `cli_recall_max_chars` failure is fixed.
- `cargo test --workspace` (plain) compiles: the broken `knn-probe.rs` example is gone;
  examples dir holds only `near-dup-calibrate.rs`.

## Perf snapshot (`docs/perf.md`)

- Release binaries: `remem` 12.8 MB, `remem-mcp` 12.2 MB.
- `remember` avg 248.4 ms (n=100); `recall --k 5` avg 241.1 ms (n=50).
- `validate` avg 149.4 ms; `stats` avg 150.0 ms (n=10 each).
- Embedder at measure time: Cpu 768d; per-process startup (~150 ms) dominates cost.

## Install

- Build from source: `cargo build --release` (binaries `target/release/remem`, `remem-mcp`).
- Model dir: `/home/bindesh/rag/cadet-embed-base-v1`, override with `REMEM_EMBED_MODEL_DIR`.
- DB path: `REMEM_DB` (default `~/.remem/remem.db`); demo: `scripts/demo.sh`.
- MCP registration: see `docs/prime-agent-integration.md` (`prime-agent mcp add local ... remem-mcp`).

## Known gaps

- Post-RRF multipliers still outvote dual fts#1+vector#1: fused 31/37 @1 vs vector-alone
  34/37; success criterion (fused >= 34/37) unmet (tracker: open; `docs/ranking-study.md`).
- Score floor default off (`0.0` = off); adversarial junk (~0.012-0.016) needs
  `--min-score 0.02`, which also drops the two weakest rank-5 tag anchors (tracker #1).
- Near-duplicate report on write: `similar:` line (CLI) / `similar` array (MCP) landed
  (`SIMILAR_MAX_DISTANCE` 0.48, floor stays opt-in); tracker #2 closed.
- CLI forget/related/central/path landed (tracker #3 closed); MCP recall `since`/`until`
  also landed. MCP `remember occurredAt` landed (tracker #4 closed).
- Score floor default stays off (`0.0` = off); tracker #1 closed.
- Tag-anchor ranking beyond the 1.05x prefix boost (Q27/Q28, tracker #6 open); MCP nits
  (case-sensitive `Content-Length`, `related` vs `link` unknown-id consistency).
- GPU build blocked upstream (tracker #5 open); year-bound checked dates landed.
- GPU build blocked upstream: candle-kernels `compatibility.cuh` vs CUDA 13 on sm_75
  (see `docs/cuda.md`).
- Gemini provider down (semaphore timeouts); recall built on qwen instead.
