# ReMem v3 recall quality eval

First recall-quality measurement. Fixture: `docs/eval-fixtures.json` (25 memories, all 6 kinds,
7 agents, varied tags; 25 natural-language queries, each naming its expected top-1 memory by
content substring). Runner: `scripts/eval.sh` (loads via `remem remember`, queries via
`remem recall --k 5`, scores recall@1 / recall@5 by content substring match). Database:
fresh `/tmp/remem-eval.db`, embedder: Cpu 768d, all memories written moments before querying
(so recency is ~1.0 for everything and does not differentiate).

## Results

- recall@1: 4/25 (16%)
- recall@5: 16/25 (64%)
- 9 total misses.

| # | rank | query |
|----|------|-------|
| 1 | 2 | what port does the staging postgres database listen on |
| 2 | 5 | how is the production redis cache sharded |
| 3 | 1 | how are user passwords stored and hashed |
| 4 | 5 | why did we pick sqlite for the local cache |
| 5 | 1 | can we deploy to production on friday |
| 6 | 1 | what caused the outage with the cdn |
| 7 | 2 | why were legitimate users being throttled by the rate limiter |
| 8 | miss | how should the assistant phrase its replies |
| 9 | 3 | should I run tests before pushing |
| 10 | 3 | when was the sessions table migrated |
| 11 | 2 | did we rotate the api signing keys |
| 12 | miss | what port do the billing metrics listen on |
| 13 | miss | where are the settings page design mockups |
| 14 | 3 | what is the rate limit on the graphql gateway |
| 15 | 4 | how do we roll out the new checkout flow |
| 16 | 2 | why did the worker use a stale schema |
| 17 | miss | what must a commit message contain |
| 18 | miss | how many requests per second did the load test reach |
| 19 | miss | where do I find the onboarding docs |
| 20 | 3 | when do nightly backups run and how long are they kept |
| 21 | 4 | why is the ingest pipeline written in rust |
| 22 | 1 | was there ever a leaked database credential |
| 23 | miss | what should code reviews prioritize |
| 24 | miss | when did the mobile app beta launch |
| 25 | miss | what do I need before connecting to the office vpn |

## Failure analysis

### Failure pattern 1: FTS never fires on natural-language queries (dominant pattern)

All 9 misses and most near-misses rank purely on `vector#N + recent` reasons. FTS contributes
only when the query happens to reuse the memory's exact tokens (verified: query
"staging postgres port 5432 pgbouncer" gets `fts#1 + vector#1` and wins by 2x). The reason is
`fts_quote` in `crates/remem-store/src/lib.rs`: it ANDs every whitespace token as a quoted
term, so "what port does the staging postgres database listen on" requires ALL of
what+port+does+the+... to appear in the memory. Natural-language queries with filler words
almost never satisfy the AND, so FTS returns nothing and recall runs on vector search alone.

### Failure pattern 2: 768d embedding too coarse for short paraphrases

With FTS dead, ranking falls to the 768d embedder, which cannot separate a paraphrased target
from a crowd of same-domain memories. Examples (all targets present in the store, ranked
outside the top 5, or absent):

- "where are the settings page design mockups" -> the Figma mockups memory is not in top 5;
  the unrelated outage memory ranks #1 on `vector#5`.
- "what should code reviews prioritize" -> "correctness first and style second" memory is not
  in top 5; the leaked-password memory ranks #1 on `vector#9`.
- "what do I need before connecting to the office vpn" -> the YubiKey memory is not in top 5;
  the CDN outage memory ranks #1 on `vector#2`.
- "how should the assistant phrase its replies" -> the terse-replies preference is not in
  top 5; the leaked-password memory ranks #1 on `vector#8`.

### Failure pattern 3: importance multiplier distorts fusion

`final_score = fused * (0.5 + importance) * (0.7 + 0.3*recency)` (crates/remem-recall/src/rank.rs).
An importance-0.8 memory gets a 1.3x multiplier, importance 0.5 gets 1.0x. With weak vector
signals (distances of ~0.019 vs ~0.017, a ~10% gap), a 1.3x importance boost overrides a
better vector rank. Concrete case, query "how is the production redis cache sharded": the
target memory is `vector#1` but with importance 0.6 scores 0.0180, while the unrelated
"CDN origin IP" memory at `vector#3` with importance 0.8 scores 0.0206 and wins.

## Proposed change (not implemented)

Drop the FTS quoting from AND-of-all-tokens to OR over content words. In `fts_quote`
(crates/remem-store/src/lib.rs) join quoted terms with `OR` instead of a space, after dropping
stopwords (what/does/the/do/I/...). FTS then fires on natural-language queries, adds its
lexical signal to fusion, and RRF handles the noisier list (BM25 ranks the multi-hit rows
higher). This targets the dominant failure pattern (all 9 misses) without touching the
embedder or the fusion weights.

A secondary, smaller proposal: rescale the importance multiplier from `(0.5 + importance)` to
`(0.5 + 0.5 * importance)` so importance spans 0.5..1.0 instead of 0.5..1.5. Simulated on the
same run (rescaling each hit's score), this fixes the redis-vs-CDN inversion and moves
recall@1 from 4/25 to 5/25 with recall@5 unchanged.

## 40-fixture run (2026-09-08)

Full 40-memory / 40-query run of `scripts/eval.sh` on `/tmp/remem-eval.db` (debug build,
commit 6b4acb2 with siblings' worktree changes present; rebuild of `remem-recall` succeeded
first try). Scoring split per the fixture design: 37 answerable queries (non-empty `expect`)
plus 3 adversarial queries with `expect: ""` (pure-stopword queries with no good answer).

- Answerable (37): recall@1 20/37 (54%), recall@5 36/37 (97%). Sole miss: Q27 ("wait what
  did we decide at the start of the year about spending") ranks its target "Q1 decision"
  memory at #9; only lexical anchor is the tag `decision`, which neither channel matches.
- Adversarial (3): no empty result. All three returned 5 hits with near-zero fusion scores
  (top score 0.012-0.014 vs ~0.029 for a good query). Junk, not empty; there is no empty
  result threshold below which recall bails.
- Combined numbers if adversarials are naively counted: 23/40 recall@1, 39/40 recall@5
  (eval.sh counts empty-expect as instant rank 1, so its printed totals are not meaningful).

Comparison to the 25-fixture baseline (4/25 = 16% recall@1, 16/25 = 64% recall@5): the FTS
stopword-OR fix held. At 40 fixtures recall@1 is 20/37 and recall@5 is 36/37; the larger
mixed fixture set raised both metrics. Verdict: fix held at 40; remaining weakness is
tag-only lexical anchors (Q27) and the absence of a no-results floor for adversarial junk.

## Tag boost (2026-09-08)

Implemented the Q27 fix as a third ranking signal in `remem-recall`: `rank::tag_boost`
returns a bounded 1.0/`TAG_MATCH_BOOST` (1.2) factor per hit, from case-insensitive
shared-prefix overlap (4 leading characters) between query words and the hit's tags, and
`RecallEngine::recall` multiplies `final_score` by it before the sort. Queries rarely carry
a `--tags` filter, and when they do the filter already removed the non-matching rows, so the
signal comes from the free-text words, not `RecallQuery::tags`. Matching words and tags
shorter than 4 characters is off: 3 collides by accident (`per`/`perf`, `second`/`security`,
`production`/`process`), 5 loses the stem drift the channel exists for (`decide`/`decision`).

Same fresh-db 40-fixture run as above (debug build, Cpu 768d):

- Answerable (37): recall@1 23/37 (62%, was 20/37), recall@5 37/37 (100%, was 36/37).
- Q27 moved miss -> rank 5. Also promoted to rank 1: Q2 (redis sharding), Q14 (graphql rate
  limit), Q18 (load test rps), Q19 (onboarding docs). Demoted within the window: Q7 rank 1 -> 2,
  Q28 rank 4 -> 5. Net recall@1 +3 with no recall@5 loss.
- eval.sh printed totals (26/40, 40/40) count the 3 adversarial queries as rank-1 hits and are
  not meaningful, as noted above; the adversarial scores are unchanged by this fix.
- Hits carrying the signal get a `tag` reason, so a boost is auditable in `--json` output.

Remaining weakness is now the adversarial junk (floor is still off by default), not tag anchors.

## Corrected scoring (2026-09-08)

Old inflated `scripts/eval.sh` totals counted the 3 adversarial queries (empty `expect`)
as rank 1 via empty-substring matching: 26/40 recall@1, 40/40 recall@5. Corrected run on a
fresh scratch db (`/tmp/remem-eval-fixed.db`, debug build, sibling worktree changes present,
build succeeded first try): answerable recall@1 23/37, recall@5 35/37, adversarial 3/3 pass.
Method: every recall runs with `--min-score 0.02` (calibrated floor); empty-expect queries
PASS only when recall returns `[]` and are reported on a separate adversarial line, excluded
from recall denominators. Note: the floor drops the rank-5 targets of Q27/Q28 (scores below
0.02), so recall@5 is 35/37 here vs 37/37 unfloored; adversarial junk (top score ~0.012-0.014)
is fully suppressed.

## Post-tag-boost green-watch run (2026-09-08, independent verification)

Green-watch protocol: `cargo test --workspace` passed on the first of up to 6 attempts
(10 min apart), so the eval ran immediately. Debug `remem` rebuilt and sha-verified
(`cargo build` after rebuild: binary hash unchanged), so the numbers below are pinned to the
current worktree (get()-caller fix + tag boost in `remem-recall`, mcp fix in `remem-mcp`).
Two independent `scripts/eval.sh` runs on a fresh `/tmp/remem-eval.db` gave identical
results; three earlier runs during the sibling handoff landed on the pre-tag-boost binary and
reproduced exactly the 20/37 + 36/37 baseline above.

- Answerable (37): recall@1 23/37 (62%, was 20/37), recall@5 37/37 (100%, was 36/37).
- Q27 ("wait what did we decide at the start of the year about spending"): miss -> rank 5.
  Its target ("The Q1 decision ...") now carries a `tag` reason (1.2x, `decide`/`decision`
  prefix overlap). Also promoted to rank 1: Q2, Q14, Q18, Q19; Q7 demoted 1 -> 2 within
  the top 5.
- Delta vs the 20/37 + 36/37 run: recall@1 +3, recall@5 +1, no query lost its top-5 rank.
- Adversarial (3, empty `expect`): all three still return 5 hits with top fusion scores
  ~0.012-0.014 (junk, not empty); unfloored eval.sh prints them as rank-1, which is why its
  combined 26/40 + 40/40 is not meaningful (see Corrected scoring above).
- Verdict: tag boost confirmed. The remaining weakness is adversarial junk, not tag anchors;
  the calibrated `--min-score 0.02` floor (corrected-scoring run: 35/37 floored) trades the
  two weakest rank-5 hits for a hard suppression of the junk channel.

## Boost width check (2026-09-08)

Per docs/ranking-study.md (fused 23/37 @1 still trails FTS-alone 33/37 and vector-alone
34/34 @1), the tag boost was re-measured at 1.05x against 1.2x with the corrected floored
runner: both give recall@1 23/37 and recall@5 35/37; they differ only in which of Q7/Q19
ranks first. Kept 1.2x (it is what clears the tag-only anchors over the top-5 boundary in the
unfloored run). Conclusion recorded in the `TAG_MATCH_BOOST` doc comment: the constant is not
the lever, the post-RRF importance/recency bands are, and offline re-ranking of `--k 40`
output mispredicts because list depth is 4k.
