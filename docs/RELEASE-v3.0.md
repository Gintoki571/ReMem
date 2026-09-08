# remem v3.0 release notes — DRAFT

Status: DRAFT. Branch `v3`, unreleased. Docs only, no API freeze claimed.

## Highlights

- Single-file SQLite memory engine: tables + FTS5 + sqlite-vec + graph projection.
- RRF recall fusing vector + FTS with recency/importance weighting and tag boost.
- Content-hash dedup on write; hard purge removes row, FTS entry, and embedding.
- Token-budget packing (`--max-chars`, top hit never dropped).
- Graph links with `graph#` recall reasons; `related`/`central`/`path` ops.
- `validate` health check (empty output means healthy); `stats` JSON counts.
- MCP server over stdio: remember/recall/list/link/forget/purge/stats/validate/related.
- `remember`/`recall`/`list`/`link`/`purge`/`stats`/`validate` CLI on `remem` binary.
- Onboarding demo (`scripts/demo.sh`) and eval harness (`scripts/eval.sh`).
- CPU-by-default local embeddings (768d); CUDA opt-in via feature flag.

## Quality numbers

- Eval (40 fixtures, 37 answerable + 3 adversarial; `docs/eval.md`): 23/37 @1 (62%), 37/37 @5 (100%).
- Floored run (`--min-score 0.02`): 23/37 @1, 35/37 @5, adversarial 3/3 suppressed.
- Baseline (25 fixtures): 4/25 @1 (16%), 16/25 @5 (64%).
- `cargo test --workspace`: 124 passed, 0 failed, 2 ignored (read-only run, this machine).
- Per crate passed: embed 4 (1 ignored); graph 24 (1 ignored); mcp 18; recall 48; store 29; types 1.

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

- GPU build blocked upstream: candle-kernels `compatibility.cuh` vs CUDA 13 on sm_75 (see `docs/cuda.md`).
- Score floor default off (`0.0` = off); adversarial junk (~0.012-0.014) needs `--min-score 0.02`.
- Gemini provider down (semaphore timeouts); recall built on qwen instead.
