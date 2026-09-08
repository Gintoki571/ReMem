# Post-RRF multiplier proposal (from docs/ranking-study.md)

## Context
Fused recall@1 22/37 vs FTS 33/37, vector 34/37; 11 dual-agreement #1s flipped.
Cause: post-RRF bands (importance 0.5..1.0, recency 0.7..1.0, tag 1.2x) outvote RRF gaps.
RRF weights untouched: signals agree at #1 on 31/37, so fusion is not the lever.

## Proposed bands
- Importance: `0.9 + 0.1 * imp` (band 0.90..1.00) — caps swing at 11% so it breaks near-ties only, never a dual #1.
- Recency: `0.9 + 0.1 * rec` (band 0.90..1.00) — same tie-break role; fresh-DB study shows recency must not differentiate when all rows are fresh.
- Tag boost: max `1.1x`, single application, only when RRF gap < 10% — keeps Q27/Q28 tag-anchor rescue without flipping Q1/Q10-style dual #1s.
- Floor: keep `apply_floor` unchanged — out of scope; this proposal only narrows multiplicative bands.

## Success criterion
Single number: fused recall@1 on `docs/eval-fixtures.json` (37 answerable) must be `>= max(FTS-alone, vector-alone)` (target: >= 34/37, no fused rank worse than worst single-signal rank).

## Risks
1. Recency flattening kills session-ordering — back-to-back edits in one session become ties; mitigate with recency half-life tune later, not band width.
2. Importance becomes decorative — high-importance pins stop surfacing on vague queries; mitigate with explicit pin/flag path outside ranking.
3. Gated tag boost regresses Q27/Q28 tag-only anchors — FTS no-match rows may drop; mitigate by keeping vector-only fallback when FTS returns no match.
