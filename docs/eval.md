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
