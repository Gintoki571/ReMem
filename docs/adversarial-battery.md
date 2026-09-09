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
