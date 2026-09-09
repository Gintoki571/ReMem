# Adversarial recall battery (live DB, read-only, 2026-09-09)

Binary as-is (debug), DB `~/.remem/remem.db` (32 memories), `recall --k 1 --json`. No writes. Scores ~0.03 across the board (floor off) — rank order only.

| # | pattern | query | top-1 (id / kind / score) | sensible |
|---|---------|-------|---------------------------|----------|
| 1 | negation | `what did we NOT decide about embeddings` | `2727448e` fact 0.0321 — persistent-mcp-server verdict | N — NOT ignored, returns unrelated fact |
| 2 | misdirection | `sqlite-vec pin MCP recall gap framing parse failure` | `97be9e19` fact 0.0319 — sqlite-vec pin 0.1.9 | Y — picks strongest component, acceptable |
| 3 | time-travel | `what did we know before March` | `397cc819` mistake 0.0324 — content_hash index ordering | N — no temporal filter from prose; arbitrary hit |
| 4 | stopword-only | `what is the and or of it` | `498511a7` fact 0.0321 — backup/restore rule | N — content-free, arbitrary hit |
| 5 | single common word | `memory` | `58071b9b` fact 0.0321 — soft-delete not erasure | N — generic term, arbitrary hit |
| 6 | very long query | 50-term kitchen sink (recall/FTS/RRF/floor/WAL/dedup/backup/…) | `99607575` decision 0.0338 — "v3 is Rust" overview | Y — generic overview is the best single answer |
| 7 | ALL CAPS | `SQLITE-VEC PIN VERSION REQUIREMENT` | `97be9e19` fact 0.0321 — sqlite-vec pin 0.1.9 | Y — case-robust, exact hit |
| 8 | typo-ridden | `sqilte-vec pn versoin requiers compil` | `97be9e19` fact 0.0321 — sqlite-vec pin 0.1.9 | Y — typo-robust, exact hit |
| 9 | empty-ish | `thing` | `397cc819` mistake 0.0162 — content_hash index ordering | N — arbitrary hit, lowest score but still returned |
| 10 | cross-kind | `mistakes about embeddings` | `2727448e` fact 0.0313 — persistent-mcp-server verdict | N — kind word ignored; a mistake (embedder trust / Rayon) expected |

Sensible: **4/10** (2, 6, 7, 8).

## Verdict — weakest pattern: content-free queries never abstain
Stopword-only, single-word, and "thing" queries all return arbitrary top-1 hits at normal-looking scores instead of nothing, so callers can't distinguish "no match" from a real answer. Fix lane: run the battery floored (`--min-score`) or surface the score gap, not new ranking logic.

## Post-guard re-run (live DB, read-only, 2026-09-09)

Guard under test: `b95af36 feat(v3): abstain on content-free queries` — `recall`
returns `[]` with reason `query-empty` when no content-bearing token remains
after stopword removal (plus `thing` treated as a stopword). Same setup as
above: debug binary, DB `~/.remem/remem.db`, `recall --k 1 --json`, no writes.
Note: the exact 50-term text of #6 was never recorded, so #6 below is a
representative reconstruction on the same themes (recall/FTS/RRF/floor/WAL/
dedup/backup/sqlite-vec/embeddings/rank/pack/graph/MCP/CLI); all other queries
are verbatim.

| # | old top-1 | new top-1 | delta | sensible now |
|---|-----------|-----------|-------|--------------|
| 1 | `2727448e` fact 0.0321 | `2727448e` fact 0.03211 | none | N — still returns unrelated fact |
| 2 | `97be9e19` fact 0.0319 | `97be9e19` fact 0.03186 | none | Y — unchanged |
| 3 | `397cc819` mistake 0.0324 | `397cc819` mistake 0.03243 | none | N — still arbitrary, no temporal filter |
| 4 | `498511a7` fact 0.0321 | `[]` (query-empty) | flipped to abstain | Y — was N |
| 5 | `58071b9b` fact 0.0321 | `58071b9b` fact 0.03211 | none | N — `memory` is a content word, guard does not fire |
| 6 | `99607575` decision 0.0338 | `99607575` decision 0.03190 | none (same id; score differs, query text reconstructed) | Y — unchanged |
| 7 | `97be9e19` fact 0.0321 | `97be9e19` fact 0.03212 | none | Y — unchanged |
| 8 | `97be9e19` fact 0.0321 | `97be9e19` fact 0.03212 | none | Y — unchanged |
| 9 | `397cc819` mistake 0.0162 | `[]` (query-empty) | flipped to abstain | Y — was N |
| 10 | `2727448e` fact 0.0313 | `2727448e` fact 0.03135 | none | N — kind word still ignored |

Sensible: **6/10** (2, 4, 6, 7, 8, 9), up from 4/10. Both flipped queries went
N -> Y via abstain-empty, with zero change to the other eight top-1 ids.

## Verdict — guard works as specified, single-word generality remains
The abstention guard fixes exactly the pattern it targets: stopword-only (#4)
and empty-ish (#9) queries now return `[]` instead of arbitrary hits. The
remaining failure modes are unchanged and out of scope for this guard:
negation (#1), time-travel prose (#3), single content-word generality (#5),
and kind-word handling (#10). Next lane: `--min-score` floor or score-gap
surfacing for low-confidence non-empty results.
