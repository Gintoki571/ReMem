# Tag-supervision autopsy: Q27/Q28 vs 5 solved tag queries (issue #6)

Method: scratch DB `/tmp/tag-sup.db` (40 fixtures, debug binary at HEAD plus
sibling's uncommitted weight WIP in `remem-recall`, Cpu 768d), `recall --k 40
--json` so `fts#N`/`vector#N` are exact full-list ranks, plus python sqlite
reads (FTS5 re-query with the shipping `fts_quote`, tag document frequency).
Current scratch state: recall@1 34/37; non-@1 are Q7 (2), Q27 (5), Q28 (4).
FTS indexes content only (`memories_fts(content)`); tags never appear in FTS.

## Per-query autopsy

| q | target tags / kind | target fts / vec | target tag? | what outranks it |
|---|--------------------|------------------|-------------|------------------|
| Q27 spend | [decision, process] / decision | none / #1 | yes (`decide`/`deci` vs `decision`, tag df 4) | 4 dual fts+vector hits (0.0276-0.0323 vs 0.0169); #1 SQLite-choice and #2 OTel carry the SAME `decision` tag and the same 1.05x boost |
| Q28 audit | [security, process] / event | none / #1 | NO — zero 4-prefix overlap (`audi/ever/back/wint/chec` vs `secu/proc`); only tag hit in top 8 is a distractor (`back`/`backup` coincidence on backups row) | 3 dual hits (0.039-0.044 vs 0.0161) |
| Q2 redis | [infra, cache] / fact | #1 / #1 | yes (`cache`, df 1) | nothing (0.0331 vs 0.0310) |
| Q5 friday | [deploy, process] / decision | #1 / #1 | yes (`deploy`, df 2) | nothing (0.0334 vs 0.0307) |
| Q30 analytics | [decision, analytics] / decision | #1 / #1 | yes (`analytics`, df 1) | nothing (0.0334 vs 0.0157) |
| Q20 backups | [ops, backup] / fact | #1 / #1 | yes (`backups`/`backup`, df 1) | nothing (0.0331 vs 0.0269) |
| Q31 pipeline | [incident, pipeline] / event | #1 / #1 | yes (`pipeline`, df 2) | nothing (0.0331 vs 0.0294) |

Discriminating feature: dual-list agreement plus match rarity. Every solved
target holds fts#1 AND vector#1 (RRF ~0.033) with a tag match on a rare tag
(df 1-2); the boost is decorative. Q27's only anchor is a COMMON tag
(`decision`, df 4) shared with 2 of its 4 above-ranked distractors, so the
binary 1.05x fires on both sides and cannot discriminate; the 2x single-vs-dual
RRF gap (0.016 vs 0.028+, FTS can never match a tag-only anchor) dominates.
Q28 is worse: its anchor (`auditors`/`audit`) lives in content, matches no tag,
and quoted-FTS misses the stem (`"auditors"` vs content `audit`), so the tag
channel is inapplicable by construction and only vector#1 remains.
Matrix-wide there are just 17 fts-absent+tag non-target hits (mostly deep:
Q16 `worker`/`workflow` collisions), and all 34 current @1 targets have fts
presence. Offline rescore over all 37 queries: removing the boost from
fts-present hits alone fixes Q7 (2->1); a gated boost on fts-absent hits at
2.0x puts Q27 at 0.0323 (above the 0.02 floor, still below dual rivals, rank
3-4); nothing else moves at any gain up to 3.0x, where the miss set is {Q28}.

## Proposal (one)

Gate the tag factor to FTS-absent hits and pay it at 2.0x. In `rank.rs`, change
`tag_boost` to take per-hit FTS presence (or split the call in `lib.rs`
`recall` where `entry.reasons` already records `fts#N`): factor is 1.0 for any
hit with an `fts#N` reason, 2.0 for a hit with none plus a query-word/tag-word
4-prefix match, unchanged matching rule otherwise. 2.0x is the single-vs-dual
RRF compensation (1/61 vs 2/61), not a tuned constant.
Predicted effect (offline rescore, same 37 queries): recall@1 34/37 -> 35/37
(Q7 fixed by the gating, Q27 5->3 unfloored and its score 0.0323 clears the
0.02 floor so floored recall@5 goes 35/37 -> 36/37); Q28 unchanged by design
(no tag overlap exists to reward — it needs tags in the FTS corpus or unquoted
stemming, a store-side fix outside `rank.rs`).
Cannot break the solved: all 34 current @1 targets carry fts presence, so the
gate leaves their scores byte-identical except rivals losing an override-only
1.05x (which restores fusion order, per the Q7 evidence); the only hits that
gain are the 17 fts-absent+tag rows, and the full-matrix sweep shows zero of
them leapfrogging a target at gains up to 3.0x.
