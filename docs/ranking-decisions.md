# Ranking decisions (live settings)
Source studies: weight-spike, temporal-eval, ranking-study, tag-supervision, floor-decision.

1. RRF k=30 (rejected: 60, 120) — weakest lever, 18/36 top-tie winners at k=30; take smallest. (weight-spike.md: top-5)
2. Fusion weights FTS 1.0 / vec 0.5 (rejected: 1:1, vec>=FTS) — all 36 winners have FTS>=vec; lets lexical signal decide. (weight-spike.md: winner; ranking-study.md: verdict 1-3)
3. Importance off, factor 1.0 (rejected: narrow 0.9+0.1x, wide 0.5+0.5x) — wide best is 26/37; 0.8-importance distractors outvote dual fts#1+vec#1 (Q7/Q12/Q22/Q36). (weight-spike.md: winner; ranking-study.md: verdict 2,4)
4. Recency kept at 0.9+0.1x (rejected: off, widened) — real spread moves ranks but buys 0 top-1s, costs 1 (flat 6/7 vs default 5/7); aged-vs-aged band is 0.49%; "off" only reflects timeless fixtures. (temporal-eval.md: verdict; weight-spike.md: overfit cautions)
5. Tag boost gated 2.0x on FTS-absent hits only (rejected: 1.05x always-on, fully off) — always-on costs more than it fixes (best tag-on 33/37); 2.0x compensates single-vs-dual RRF gap (1/61 vs 2/61); gating fixes Q7, lifts Q27 over floor, Q28 unchanged by design. (tag-supervision.md: proposal; weight-spike.md: winner)
6. Score floor --min-score 0.02 opt-in, default 0 (rejected: always-on 0.02, tuned 0.0157) — true targets (0.0159+) overlap junk (to 0.0156); always-on trades 2 true hits (Q27/Q28) for noise suppression. (floor-decision.md)
7. Abstention on: return [] when all hits below floor / adversarial stopword queries (rejected: always return top-k) — floored recall keeps 31/37 @1 while adversarial goes 0/3 to 3/3 suppressed. (floor-decision.md; ranking-study.md: 3 adversarial excluded)
