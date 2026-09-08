# Ranking study: FTS-alone vs vector-alone vs fused (1:1 RRF)

Fixture: `docs/eval-fixtures.json` (40 memories, 40 queries; 37 answerable, 3 adversarial
pure-stopword queries with `expect: ""` excluded from ranks). Scratch DB `/tmp/rank-mem.db`,
fresh debug `remem` (first-try green, commit `ec89a73` + dirty worktree: sibling mid-edit in
`remem-recall` lib.rs/rank.rs/engine tests — fused numbers reflect that worktree).
Embedder: Cpu 768d BERT. `recall --k 40` so list depth (4k=160) covers all 40 memories and
`fts#N` / `vector#N` reasons are the exact full-list FTS (BM25) and vector (KNN L2) ranks;
python sqlite FTS5 re-query with the same `fts_quote` agreed on all 37 (0 mismatches).
Note: "fused" below = RRF + `final_score` importance/recency multipliers + 1.2x tag boost,
not pure RRF. Recency is ~1.0 for all rows (fresh DB) and does not differentiate.

| # | query | fts | vec | fused | fused reasons |
|---|-------|-----|-----|-------|---------------|
| 1 | what port does the staging postgres database listen on | 1 | 1 | 3 | fts#1 vector#1 recent |
| 2 | how is the production redis cache sharded | 1 | 1 | 1 | fts#1 vector#1 recent tag |
| 3 | how are user passwords stored and hashed | 1 | 1 | 1 | fts#1 vector#1 recent important |
| 4 | why did we pick sqlite for the local cache | 1 | 1 | 1 | fts#1 vector#1 recent |
| 5 | can we deploy to production on friday | 1 | 1 | 1 | fts#1 vector#1 recent tag |
| 6 | what caused the outage with the cdn | 1 | 1 | 2 | fts#1 vector#1 recent important |
| 7 | why were legitimate users being throttled by the rate limiter | 1 | 1 | 2 | fts#1 vector#1 recent |
| 8 | how should the assistant phrase its replies | 1 | 4 | 2 | fts#1 vector#4 recent |
| 9 | should I run tests before pushing | 1 | 1 | 1 | fts#1 vector#1 recent tag |
| 10 | when was the sessions table migrated | 1 | 1 | 2 | fts#1 vector#1 recent tag |
| 11 | did we rotate the api signing keys | 1 | 1 | 4 | fts#1 vector#1 recent |
| 12 | what port do the billing metrics listen on | 1 | 1 | 3 | fts#1 vector#1 recent |
| 13 | where are the settings page design mockups | 1 | 1 | 1 | fts#1 vector#1 recent tag |
| 14 | what is the rate limit on the graphql gateway | 1 | 1 | 1 | fts#1 vector#1 recent tag |
| 15 | how do we roll out the new checkout flow | 1 | 1 | 1 | fts#1 vector#1 recent |
| 16 | why did the worker use a stale schema | 1 | 1 | 1 | fts#1 vector#1 recent |
| 17 | what must a commit message contain | 1 | 1 | 1 | fts#1 vector#1 recent |
| 18 | how many requests per second did the load test reach | 1 | 1 | 1 | fts#1 vector#1 recent tag |
| 19 | where do I find the onboarding docs | 1 | 1 | 1 | fts#1 vector#1 recent tag |
| 20 | when do nightly backups run and how long are they kept | 1 | 1 | 1 | fts#1 vector#1 recent tag |
| 21 | why is the ingest pipeline written in rust | 1 | 1 | 1 | fts#1 vector#1 recent tag |
| 22 | was there ever a leaked database credential | 1 | 2 | 2 | fts#1 vector#2 recent important |
| 23 | what should code reviews prioritize | 1 | 1 | 2 | fts#1 vector#1 recent |
| 24 | when did the mobile app beta launch | 1 | 1 | 1 | fts#1 vector#1 recent tag |
| 25 | what do I need before connecting to the office vpn | 1 | 1 | 4 | fts#1 vector#1 recent |
| 26 | hey so remember when things went down earlier in the year, like that whole march thing | 3 | 1 | 1 | fts#3 vector#1 recent important |
| 27 | wait what did we decide at the start of the year about spending | None | 1 | 5 | vector#1 recent tag |
| 28 | did the auditors ever get back to us about that winter check | None | 1 | 6 | vector#1 recent |
| 29 | there was something with invoices being late a while back, when was that | 2 | 1 | 1 | fts#2 vector#1 recent important |
| 30 | what was the plan again for moving analytics somewhere else | 1 | 1 | 1 | fts#1 vector#1 recent tag |
| 31 | how long was the pipeline down during that summer maintenance thing | 1 | 1 | 1 | fts#1 vector#1 recent tag |
| 32 | when does the old public api go away exactly | 1 | 1 | 1 | fts#1 vector#1 recent important |
| 33 | why did the worker end up with an outdated schema | 1 | 2 | 1 | fts#1 vector#2 recent |
| 34 | didn't we have a credential sitting in a config file once | 1 | 1 | 1 | fts#1 vector#1 recent important |
| 35 | so um i was wondering like where exactly do those mirrors live or whatever | 1 | 1 | 2 | fts#1 vector#1 recent |
| 36 | ok so basically where do the writeups about incidents go and stuff | 1 | 1 | 2 | fts#1 vector#1 recent |
| 37 | hmm what was that whole thing about tracing and standards for new services | 1 | 1 | 4 | fts#1 vector#1 recent |

## Counts (n=37 answerable)

- FTS-alone: recall@1 33/37 (89%), recall@5 35/37 (95%), no-match 2/37 (Q27, Q28 — tag-only anchors).
- Vector-alone: recall@1 34/37 (92%), recall@5 37/37 (100%), no-match 0/37.
- Fused (CLI recall): recall@1 22/37 (59%), recall@5 36/37 (97%), miss 0/37 (worst rank #9: Q27).
- Both signals agree at #1 on 31/37 queries; fusion keeps #1 on only 20 of those 31.

## Verdict (5 lines)

1. 1:1 RRF is not the problem — the RRF inputs are excellent (89%/92% recall@1 alone).
2. The loser is everything after RRF: importance (0.7x..0.95x), recency, and the 1.2x tag boost routinely outvote a dual fts#1+vector#1 (e.g. Q1, Q35: fts#2+vec#30 wins on importance).
3. Neither signal should "dominate" — they agree at #1 on 31/37, so reweighting FTS vs vector barely matters; 11 of 15 fused misses had the target at fts#1 AND vec#1.
4. Fix priority: shrink post-RRF multipliers (flatten importance, gate tag boost to low-score ties) before touching fusion weights.
5. Open carve-outs: FTS no-match (Q27/Q28 tag-only anchors) is the one place vector must rule alone, and fused top-1 there still loses (#5/#6) — same multiplier cause.

