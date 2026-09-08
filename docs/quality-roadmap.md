# ReMem v3 quality roadmap (docs only)

Sources: `docs/eval.md` (40-fixture: 20/37 @1, 36/37 @5 answerable; 3 adversarial junk, no empty floor; Q27 tag-anchor miss), `docs/inspiration.md` (9 keep / 3 discard), `docs/security-review.md`, `docs/fuzz-report.md`, `docs/mcp-fuzz-report.md`, `CHANGELOG-v3.md`.

## Status table (gap | severity | status | owner)

| gap | severity | status | owner |
|---|---|---|---|
| FTS stopword-OR fix (dominant miss pattern) | high | done | remem-store |
| Importance rescale (0.5+0.5*importance) | medium | done | remem-recall |
| Two-clock `occurred_at` event time | medium | done | remem-types |
| Token-budget packing (`--max-chars`, top hit kept) | medium | done | remem-recall |
| Score floor (`apply_floor`) | medium | done | remem-recall |
| Hard purge (row + FTS + vector) | medium | done | remem-store |
| Content-hash dedup on write | medium | done | remem-store |
| MCP tools (validate/forget/purge/related, annotations) | medium | done | remem-mcp |
| MCP hardening (16 MiB cap, k clamp, per-message errors) | medium | done | remem-mcp |
| Private perms (dirs 0700, DB 0600) | low | done | remem-store |
| Tag boost for tag-only anchors (Q27) | medium | in-flight | remem-recall |
| Graph algos (`central`, `shortest_path`) | low | in-flight | remem-graph |
| Architecture doc | low | in-flight | remem-recall |
| Floor default-off decision (0.0 = off ok?) | low | open | remem-recall |
| Near-duplicate report on write (`similar` neighbours) | medium | open | remem-recall |
| Tag-anchor miss (Q27 "decision" only in tag, rank #9) | medium | open | remem-recall |
| Adversarial empty-expect handling (junk scores 0.012-0.014, no empty floor) | medium | open | remem-recall |
| Importance validation (999/-5/NaN/inf stored; `"high"` silently ignored) | medium | open | remem-recall |
| Recall `k=0` quirk (returns hits, should be `[]`) | low | open | remem-recall |

## Notes

- Discarded per inspiration.md: cross-encoder rerank, LLM entity extraction, dual-level keyword modes.
- Open validation cluster also covers `--min-score NaN` silent `[]` and empty text/query accepted; fix with the importance-validation owner.
- MCP open nits (same owner remem-mcp): non-object JSON silent drop, `related` vs `link` unknown-id inconsistency, case-sensitive Content-Length.
