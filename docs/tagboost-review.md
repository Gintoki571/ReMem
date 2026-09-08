# Tag boost review (uncommitted diff in crates/remem-recall)

Verdict: REVISE

1. Bounded PASS: `TAG_MATCH_BOOST = 1.05` (rank.rs:121), single factor per hit (rank.rs:159-165), test asserts no stacking (rank.rs:389-395).
2. Bands NOT narrowed FAIL: `final_score` still `(0.5 + 0.5*imp) * (0.7 + 0.3*rec)` (rank.rs:105-107); proposal wants `0.9 + 0.1*` for both (docs/multiplier-proposal.md).
3. Doc/code mismatch: code is 1.05 (rank.rs:121) but docs/eval.md boost-width section and quality-roadmap claim kept/measured 1.2x; ranking-study says fused includes 1.2x.
4. Case-fold one-sided: `tokens()` lowercases query (rank.rs:132-135) but `shares_prefix` compares raw tag bytes (rank.rs:172-176); tag `Decision` misses query `decide`.
5. Non-ASCII panic risk: byte slice `a[..TAG_MIN_PREFIX]` (rank.rs:176) panics on multibyte boundary; use char-based prefix or `get()`.
6. Order correct: boost (lib.rs:299) before sort/truncate/pack/floor, floor last (lib.rs:303-310); sign correct (`*= factor`, `factor > 1.0`).
7. Ungated vs proposal: boost applies whenever prefix matches, not only when RRF gap < 10% as proposed; ok at 1.05x but deviates from the gate.
8. Condition for GO: narrow `final_score` bands per proposal, fix findings 3-5, re-run eval fixtures to confirm fused recall@1.
