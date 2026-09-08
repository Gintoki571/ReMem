# ReMem v3 quality roadmap (docs only)

Sources: `docs/eval.md` (40-fixture: 20/37 @1, 36/37 @5 answerable; 3 adversarial junk, no empty floor; Q27 tag-anchor miss), `docs/inspiration.md` (9 keep / 3 discard), `docs/security-review.md`, `docs/fuzz-report.md`, `docs/mcp-fuzz-report.md`, `CHANGELOG-v3.md`.

Verified against worktree/HEAD before marking done (HEAD `ede3ba6`, plus uncommitted MCP central/path work).

## Status table (gap | severity | status | owner)

| gap | severity | status | owner |
|---|---|---|---|
| FTS stopword-OR fix (dominant miss pattern) | high | done | remem-store |
| Importance rescale (0.5+0.5*importance) | medium | done | remem-recall |
| Two-clock `occurred_at` event time | medium | done | remem-types |
| Token-budget packing (`--max-chars`, top hit kept) | medium | done | remem-recall |
| Score floor (`apply_floor`; MCP `minScore`/`maxChars` passthrough) | medium | done | remem-recall |
| MCP coercion errors (`k`/`importance`/`minScore`/`limit` strings -> `isError`) | medium | done | remem-mcp |
| Strict row decoding (unknown kind / unparseable tags -> `Err`, names row) | medium | done | remem-store |
| Hard purge (row + FTS + vector; CLI + MCP) | medium | done | remem-store |
| Content-hash dedup on write | medium | done | remem-store |
| MCP tools (`purge`/`related`/`central`/`path`, annotations) | medium | done (central/path worktree, uncommitted) | remem-mcp |
| MCP hardening (16 MiB cap, k clamp, per-message errors) | medium | done | remem-mcp |
| Graph algos (`central`, `shortest_path` + lib tests) | low | done | remem-graph |
| Private perms (dirs 0700, DB 0600) | low | done | remem-store |
| Tag boost for tag-only anchors (Q27) | medium | in-flight | remem-recall |
| Architecture doc (draft `docs/architecture-v3.md`, untracked) | low | in-flight | remem-recall |
| `Store::get` signature break (returns `Result`; recall/mcp callers still `Option`-style) | high | in-flight | remem-recall |
| MCP `central`/`path` tests green (blocked on `Store::get` fix) | medium | in-flight | remem-mcp |
| Floor default-off decision (0.0 = off ok?) | low | open | remem-recall |
| Near-duplicate report on write (`similar` neighbours) | medium | open | remem-recall |
| Tag-anchor miss (Q27 "decision" only in tag, rank #9) | medium | open | remem-recall |
| Adversarial empty-expect handling (junk scores 0.012-0.014, no empty floor) | medium | open | remem-recall |
| Importance validation (999/-5/NaN/inf stored; `"high"` silently ignored) | medium | open | remem-recall |
| Recall `k=0` quirk (returns hits, should be `[]`) | low | open | remem-recall |

## Notes

- `Store::get` now returns `Result<Option<..>>` (strict decoding); `remem-recall/src/lib.rs:264` and `remem-mcp/src/main.rs:324` still treat it as `Option`, so recall/MCP do not compile until the fix lands.
- Discarded per inspiration.md: cross-encoder rerank, LLM entity extraction, dual-level keyword modes.
- Open validation cluster also covers `--min-score NaN` silent `[]` and empty text/query accepted; fix with the importance-validation owner.
- MCP open nits (same owner remem-mcp): non-object JSON silent drop, `related` vs `link` unknown-id inconsistency, case-sensitive Content-Length.
