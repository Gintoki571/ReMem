# Weight spike: offline grid search over fusion weights and multiplier bands

## Method

Fresh scratch DB `/tmp/spike.db` built with the debug `remem` binary
(real Cpu 768d embedder): all 40 `docs/eval-fixtures.json` memories
remembered with their fixture importance/tags. For each of the 37
answerable queries, `remem recall --k 40 --json` returned the full
candidate set (depth 4k covers all 40 rows), and the `fts#N` / `vector#N`
reasons give exact full-list FTS (BM25) and vector (KNN L2) ranks.
No `graph#N` reason appears in any of the 1480 hits: the only edges are
`BELONGS_TO_AGENT` (memory to agent node), so graph expansion yields no
memory neighbours and the graph weight is a no-op on these fixtures.

Per-candidate features come from python sqlite reads: embeddings verified
as 40 rows of 768 float32 LE (3072-byte blobs, L2 norm 1.0); importance,
tags, and timestamps from `memories`; recency from
`0.5^(age_days/30)` off `occurred_at ?? created_at` (range 0.9999..0.9999:
timeless fixtures, recency carries no signal); tag-match flag recomputed
with the exact rank.rs rule (4-char prefix, 43 flagged candidates).

Grid (729 configs, graph weight fixed): RRF k in {30, 60, 120}, FTS and
vector weights in {0.5, 1, 2}, importance / recency / tag bands in
{off (factor 1.0), narrow (0.9+0.1x), wide (0.5+0.5x)} where x is
importance, recency, or the 0/1 tag flag. Score is fused recall@1 over
the 37 queries by content-substring match, no score floor.

Sanity: recomputing the current code path (k=60, 1/1 weights, narrow
importance/recency, 1.05x tag boost) reproduces the binary order exactly
(0/37 order mismatches) and scores 31/37, matching the verified
post-merge fused number in `docs/ranking-study.md`.

## Top-5 configs (all tie at 35/37; 36 configs total share the top score)

| # | RRF k | w_fts | w_vec | importance | recency | tag | recall@1 |
|---|-------|-------|-------|------------|---------|-----|----------|
| 1 | 30 | 1.0 | 0.5 | off | off | off | 35/37 |
| 2 | 30 | 2.0 | 0.5 | off | off | off | 35/37 |
| 3 | 60 | 2.0 | 1.0 | off | off | off | 35/37 |
| 4 | 30 | 1.0 | 0.5 | narrow | narrow | off | 35/37 |
| 5 | 120 | 2.0 | 1.0 | off | wide | off | 35/37 |

Current baseline: 31/37. Delta: +4 (83.8% to 94.6%).

Contrast points (same grid): best config with any tag boost on is 33/37
(k=30, 2.0/1.0, imp off, tag narrow); best config with wide importance is
26/37; the worst config overall scores 15/37 (k=120, 0.5 vector weight
with wide importance). Every one of the 36 winners has tag off,
importance off-or-narrow (never wide), and FTS weight at or above vector
weight (pairs (1.0,0.5), (2.0,0.5), (2.0,1.0)); RRF k is the weakest lever
(18 winners at k=30, 9 each at 60 and 120).

## Winner

Row 1: RRF k=30, weights FTS 1.0 / vector 0.5, importance off, recency
off, tag off. Predicted recall@1: 35/37.

It fixes 4 of the 6 baseline misses: Q7 (rate limiter), Q12 (billing
port 9102), Q22 (leaked credential), Q36 (incident writeups). In each
case the target sits at fts#1 (+vector#1 except Q22/Q36) and loses today
to a wrong memory riding high importance (0.8) or a tag boost; dropping
the multipliers and down-weighting vector lets the lexical signal decide.
Q27 and Q28 (tag-only anchors, FTS no-match) still miss: the tag channel
is the only thing that can lift them, but any tag boost costs more
elsewhere (best tag-on: 33/37). That tension is the remaining headroom.

## Overfit cautions

37 queries is tiny: a 36-way tie at the top means the grid cannot
distinguish these configs, and +4 could be noise fitting (a single query
is 2.7 points). Do not read row 1 as "the right weights": k=30 vs 120
and imp off vs narrow are indistinguishable here. Recency learned
nothing (all recencies round to 1.0); "recency off" must not ship as a
real change, it only reflects that these fixtures are timeless. The
graph weight learned nothing either (empty graph list). Tag-off wins
only because 35/37 queries resolve lexically; on a corpus where tag-only
anchors are common, dropping the tag channel would lose Q27-style
queries with no compensating gain. Any adoption needs a second,
time-varied fixture set with real memory-to-memory links before weights
move in `crates/remem-recall`.
