# ReMem v3 quality roadmap (docs only)

Sources: `docs/eval.md` (40-fixture corrected eval + post-merge verification), `docs/ranking-study.md`, `docs/multiplier-proposal.md`, `docs/tagboost-review.md`, `docs/cuda-unblock.md`, `docs/cli-mcp-parity.md`, `docs/demo.md`, `docs/security-review.md`, `docs/fuzz-report.md`, `docs/mcp-fuzz-report.md`, `CHANGELOG-v3.md`.

Verified against worktree/HEAD before marking done (31/37 @1 confirmed at `995c388`; tag boost 1.05x + narrowed bands landed `e844cd4`; near-dup probe + CLI forget/related/central/path + MCP since/until all landed and committed; issues #1/#2/#3/#4 closed, only #5/#6 open; refresh verified at `4844df7`).

## Status table (gap | severity | status | owner)

| gap | severity | status | owner |
|---|---|---|---|
| FTS stopword-OR fix (dominant miss pattern) | high | done | remem-store |
| Importance rescale + clamp (0..1, NaN -> 0.5, enforced on write and read) | medium | done | remem-types |
| Two-clock `occurred_at` event time (CLI `--occurred-at`) | medium | done | remem-types |
| MCP remember `occurredAt` (unix secs or YYYY-MM-DD; landed `6f9d0b3`; #4 closed) | medium | done | remem-mcp |
| Token-budget packing (`--max-chars`, top hit kept) | medium | done | remem-recall |
| Score floor (`apply_floor`; MCP `minScore`/`maxChars` passthrough) | medium | done | remem-recall |
| MCP coercion errors (`k`/`importance`/`minScore`/`limit` strings -> `isError`) | medium | done | remem-mcp |
| Strict row decoding (unknown kind / unparseable tags -> `Err`, names row) | medium | done | remem-store |
| `Store::get` signature break (recall + mcp callers fixed) | high | done | remem-recall |
| Hard purge (row + FTS + vector; CLI + MCP) | medium | done | remem-store |
| Content-hash dedup on write | medium | done | remem-store |
| MCP tools (`purge`/`related`/`central`/`path`, annotations; 11 tools total) | medium | done (committed `a935ce7`, stdio tests green) | remem-mcp |
| MCP hardening (16 MiB cap, k clamp, per-message errors, non-object JSON -> invalid request) | medium | done | remem-mcp |
| Graph algos (`central`, `shortest_path` + lib tests) | low | done | remem-graph |
| Private perms (dirs 0700, DB 0600) | low | done | remem-store |
| Architecture doc (`docs/architecture-v3.md`), CLI/MCP parity map (`docs/cli-mcp-parity.md`), runnable end-to-end demo (`docs/demo.md`, `scripts/demo.sh`) | low | done | remem-recall |
| Tag boost for tag-only anchors: landed 1.05x, single application, case-folded, char-safe prefix (`e844cd4`) | medium | done | remem-recall |
| Post-RRF multiplier rebalance: narrowed bands `0.9 + 0.1*` for importance and recency landed (`e844cd4`) | medium | done (code); verdict PENDING temporal eval (below) | remem-recall |
| Embedder thread sweep: Rayon default (all cores) optimal, 1->10.9s .. 8->1.7s per op; no code change, constrain via `RAYON_NUM_THREADS` only (`bench_threads.rs`) | low | done (no change needed) | remem-embed |
| Core open problem: post-RRF multipliers still outvote dual fts#1+vector#1; fused 31/37 @1 (verified independently at `995c388`) but still BELOW vector-alone 34/37 @1; criterion fused >= 34/37 unmet; all 6 fused non-@1 misses have the target at vec#1 | high | pending (temporal-eval verdict decides keep vs strip multipliers) | remem-recall |
| CUDA build blocked upstream (candle-kernels `__hmax_nan` vs CUDA 13.x sm_75) | medium | open (PR huggingface/candle#3909 unmerged, tracker #5; CUDA 12.6 side-by-side + fork-patch workarounds in `docs/cuda.md`) | remem-embed |
| Floor default-off decision (0.0 = off; adversarial junk 0.012-0.016 passes unfloored) | medium | done (#1 closed: floor stays opt-in, `--min-score 0.02` calibrated) | remem-recall |
| Near-duplicate report on write: landed and committed (knn k=3, `SIMILAR_MAX_DISTANCE` 0.48 calibrated over 780 fixture pairs via `near-dup-calibrate.rs`; fires on 3/4 paraphrases, silent on all distinct pairs; CLI `similar:` line + MCP `similar` array) | medium | done (#2 closed) | remem-recall |
| CLI forget/related/central/path | low | done (#3 closed; full CLI/MCP parity `f40b529`, `docs/cli-mcp-parity.md`) | remem-recall |
| MCP recall `since`/`until` | low | done (landed in `crates/remem-mcp/src/main.rs`) | remem-mcp |
| Recall `k=0` (returned 5 hits, now returns `[]`; `5ab9995` + cli/engine tests) | low | done | remem-recall |
| CLI `list --limit` (`aed76fb` + CLI test) | low | done | remem-recall |
| Year-bound checked dates (reject out-of-range years; `66b712e` + glossary) | low | done | remem-recall |
| Reserved hub-prefix rejection (`hub:`/`hubs:`/`hubs/`; `800c668` + graph tests) | medium | done | remem-graph |
| CLI/store path hardening (HOME-unset + tilde errors name HOME, `cc27818`) | medium | done in code (CI fmt failure at HEAD, see below) | remem-recall |
| Token-lean skill (`skills/remem/SKILL.md`, 596 tokens, `4844df7`) | low | done | remem-recall |
| Tag-anchor ranking beyond the 1.05x prefix boost (Q27/Q28 supervision) | low | open (tracker #6) | remem-recall |
| MCP nits: case-sensitive `Content-Length` header, `related` vs `link` unknown-id consistency | low | open | remem-mcp |
| CI fmt failure at HEAD (`cc27818` rust job: `cargo fmt --check`; test+demo green) | low | open (code sibling owns the fix) | remem-recall |

## Notes

- Where fused stands (post-merge verification, `docs/eval.md`, fresh db at `995c388`): answerable 31/37 @1, 35/37 @5 floored, 37/37 @5 unfloored; adversarial 3/3 with the 0.02 floor. Single signals unchanged: FTS-alone 33/37 @1, vector-alone 34/37 @1. Fusion is not the loser (inputs are 33-34/37); the post-RRF multipliers still are, even narrowed to 0.9+0.1x bands.
- Multiplier question PENDING temporal-eval verdict: keep the narrowed bands vs strip to pure RRF. Do not retune constants until temporal results land.
- Boost width: 1.05x vs 1.2x measure the same recall; the constant is not the lever, the bands are. Offline re-ranking of `--k 40` output mispredicts because list depth is 4k.
- Importance validation: `clamp_importance` (0..1, NaN -> 0.5) enforced on insert/get/update; MCP `importance` strings error loudly.
- MCP surface: 11 tools over stdio; release check 2 all green on release binaries (`docs/release-check-2.md`).
- CI: green run 88 at `e5ceaee` plus `800c668` (rust+test+demo success); HEAD `cc27818` fails rust job on `cargo fmt --check` only, fix is a `cargo fmt` run by a code sibling.
- Issues: #1/#2/#3/#4 closed; only #5 (CUDA upstream) and #6 (tag-anchor supervision) open; the open rows above reference them.
- Discarded per inspiration.md: cross-encoder rerank, LLM entity extraction, dual-level keyword modes.
