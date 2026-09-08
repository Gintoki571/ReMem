# ReMem v3 quality roadmap (docs only)

Sources: `docs/eval.md` (40-fixture corrected eval), `docs/ranking-study.md` (FTS vs vector vs fused), `docs/multiplier-proposal.md`, `docs/cuda-unblock.md`, `docs/cli-mcp-parity.md`, `docs/security-review.md`, `docs/fuzz-report.md`, `docs/mcp-fuzz-report.md`, `CHANGELOG-v3.md`.

Verified against worktree/HEAD before marking done (HEAD `6f9d0b3` "MCP remember occurredAt", plus uncommitted sibling work in `remem-recall`: tag boost + width check).

## Status table (gap | severity | status | owner)

| gap | severity | status | owner |
|---|---|---|---|
| FTS stopword-OR fix (dominant miss pattern) | high | done | remem-store |
| Importance rescale + clamp (0..1, NaN -> 0.5, enforced on write and read) | medium | done | remem-types |
| Two-clock `occurred_at` event time (CLI `--occurred-at`) | medium | done | remem-types |
| MCP remember `occurredAt` (unix secs or YYYY-MM-DD; landed `6f9d0b3`, tracker #4 still open) | medium | done | remem-mcp |
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
| Architecture doc | low | done (`docs/architecture-v3.md`, tracked) | remem-recall |
| Tag boost for tag-only anchors (Q27) | medium | done in worktree, uncommitted (1.2x prefix overlap: 23/37 @1, 37/37 @5 unfloored, 35/37 @5 floored) | remem-recall |
| Post-RRF multiplier rebalance (importance/recency bands 0.9+0.1x, gated tag boost) | medium | in-flight (proposal in `docs/multiplier-proposal.md`; success criterion: fused @1 >= 34/37) | remem-recall |
| CUDA build blocked upstream (candle-kernels `__hmax_nan` vs CUDA 13.x sm_75) | medium | open (PR huggingface/candle#3909 unmerged, tracker #5; CUDA 12.6 side-by-side + fork-patch workarounds in `docs/cuda.md`) | remem-embed |
| Floor default-off decision (0.0 = off; adversarial junk 0.012-0.014 passes) | medium | open (tracker #1; corrected eval runs floored at `--min-score 0.02`) | remem-recall |
| Near-duplicate report on write (`similar` neighbours) | medium | open (tracker #2) | remem-recall |
| CLI forget/related/central/path (MCP already has them) | low | open (tracker #3; `docs/cli-mcp-parity.md`) | remem-recall |
| Tag-anchor ranking beyond the 1.2x prefix boost (Q27/Q28 supervision) | low | open (tracker #6) | remem-recall |
| Recall `k=0` quirk (returns 5 hits, should be `[]`) | low | open | remem-recall |
| MCP nits: case-sensitive `Content-Length` header, `related` vs `link` unknown-id consistency | low | open | remem-mcp |

## Notes

- Ranking study (40 fixtures, 37 answerable): FTS-alone 33/37 @1, vector-alone 34/37 @1; fused 22/37 @1 pre-tag-boost, 23/37 with the tag boost. Signals agree at #1 on 31/37, but post-RRF multipliers (importance 0.5..0.95, recency 0.7..1.0, tag) outvote dual #1s. Multiplier rebalance, not fusion weights, is the lever (`docs/ranking-study.md`).
- Corrected eval scoring (inflated `scripts/eval.sh` totals retired): every recall runs floored at `--min-score 0.02`; empty-expect adversarials pass only on `[]` and are reported on a separate line, excluded from the denominators. Corrected numbers: 23/37 @1, 35/37 @5 (the floor drops the Q27/Q28 rank-5 targets vs 37/37 unfloored), adversarial 3/3.
- Boost width check: 1.05x vs 1.2x tag boost measure the same recall (23/37 @1, 35/37 @5 floored); the constant is not the lever, the bands are. Offline re-ranking of `--k 40` output mispredicts because list depth is 4k.
- Importance validation: `clamp_importance` (0..1, NaN -> 0.5) is enforced on insert/get/update, so 999/-5/NaN/inf no longer persist raw; MCP `importance` strings error loudly.
- MCP surface: 11 tools over stdio (remember, recall, list, link, forget, purge, stats, validate, related, central, path); release check 2 all green on release binaries (`docs/release-check-2.md`).
- Discarded per inspiration.md: cross-encoder rerank, LLM entity extraction, dual-level keyword modes.
- Issues #1-#6 filed on the tracker; the open rows above reference them.
