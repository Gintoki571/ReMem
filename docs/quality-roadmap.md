# ReMem v3 quality roadmap (docs only)

Sources: `docs/eval.md` (40-fixture corrected eval + post-merge verification), `docs/ranking-study.md`, `docs/weight-spike.md`, `docs/temporal-eval.md`, `docs/multiplier-proposal.md`, `docs/tagboost-review.md`, `docs/adversarial-battery.md`, `docs/cuda-unblock.md`, `docs/cli-mcp-parity.md`, `docs/demo.md`, `docs/security-review.md`, `docs/fuzz-report.md`, `docs/mcp-fuzz-report.md`, `docs/stemmer-analysis.md`, `CHANGELOG-v3.md`.

Verified against worktree/HEAD before marking done (stemmer analysis `b62f02f` docs-only; learned weights `aebe0c2` RRF k=30 FTS 1.0/vec 0.5; floor-first `5733447` verifier GO 35/37 @1 3/3 adv 35/37 @5; store tests 22 green w/o redirect; CI success run 34326311715; issues #1-#4 closed, #5/#6 open; live db 36 memories / 41 nodes / 47 edges).

## Status table (gap | severity | status | owner)

| gap | severity | status | owner |
|---|---|---|---|
| FTS stopword-OR fix (dominant miss pattern) | high | done | remem-store |
| Tags in FTS (2-col fts5 content+tags, triggers + old-DB migration, `bm25(memories_fts, 1.0, 2.0)`; landed `a6becb3` + store tests) | medium | done | remem-store |
| Importance rescale + clamp (0..1, NaN -> 0.5, enforced on write and read) | medium | done | remem-types |
| Two-clock `occurred_at` event time (CLI `--occurred-at`) | medium | done | remem-types |
| MCP remember `occurredAt` (unix secs or YYYY-MM-DD; landed `6f9d0b3`; #4 closed) | medium | done | remem-mcp |
| Token-budget packing (`--max-chars`, top hit kept) | medium | done | remem-recall |
| Score floor (`apply_floor`; MCP `minScore`/`maxChars` passthrough; stays opt-in, default 0.0 = off) | medium | done (#1 closed; floor-first `5733447`: floor judges unboosted scores, adv back to 3/3, Q27 floor-dropped by design) | remem-recall |
| MCP coercion errors (`k`/`importance`/`minScore`/`limit` strings -> `isError`) | medium | done | remem-mcp |
| Strict row decoding (unknown kind / unparseable tags -> `Err`, names row) | medium | done | remem-store |
| `Store::get` signature break (recall + mcp callers fixed) | high | done | remem-recall |
| Hard purge (row + FTS + vector; CLI + MCP) | medium | done | remem-store |
| Content-hash dedup on write | medium | done | remem-store |
| MCP tools (`purge`/`related`/`central`/`path`, annotations; 11 tools total) | medium | done (committed `a935ce7`, stdio tests green) | remem-mcp |
| MCP hardening (16 MiB cap, k clamp, per-message errors, non-object JSON -> invalid request) | medium | done | remem-mcp |
| Graph algos (`central`, `shortest_path` + lib tests) | low | done | remem-graph |
| Private perms (dirs 0700, DB 0600) | low | done | remem-store |
| Architecture doc (`docs/architecture-v3.md`), CLI/MCP parity map (`docs/cli-mcp-parity.md`), runnable end-to-end demo (`docs/demo.md`, `scripts/demo.sh`) | low | done | remem-recall |
| CLI help text for every flag (`5c5200e`: remember kind/tags/agent/session/importance/occurred-at; recall query/k/json/agent/session/since/until; list json/limit; forget id; purge id; link from/to/rel; related id/rel; central limit; path from/to) | low | done | remem-recall |
| Tag boost for tag-only anchors: gated 2.0x on FTS-absent hits only (`b073557`); floor-first `5733447` fixed the adv regression: verifier GO 35/37 @1, 35/37 @5, 3/3 adv (Q27 floor-dropped by design, was 36/37 @5 2/3) | medium | done | remem-recall |
| Post-RRF multiplier rebalance: narrowed bands `0.9 + 0.1*` landed (`e844cd4`); learned weights LANDED (`aebe0c2` per `docs/landing-weights.md`: RRF k=30, FTS 1.0 / vec 0.5 / graph 1.0, importance out of formula, recency 0.9+0.1x, tag gate 2.0x — do not retune here) | medium | done | remem-recall |
| Weight spike (`docs/weight-spike.md`, `acfa7f4`): 729-config offline grid, winner k=30 / FTS 1.0 / vec 0.5 / multis off predicted 35/37 — MET at 35/37 @1 (fixes Q7/Q36 via gate; Q27/Q28 still miss; 36-way tie, overfit caution, needs time-varied second set) | high | done (prediction confirmed) | remem-recall |
| Temporal verdict (`docs/temporal-eval.md`, `ae5bb78`): recency stays as-is (narrow band, max 11% swing, 0.49% old-vs-old); `--since/--until` windows for time-anchored queries; future-date clamp (clamps to 1.0) flagged as open design question | medium | done (docs verdict; clamp flag open) | remem-recall |
| Pure 1:1 RRF reference: 34/37 @1 (`docs/ranking-study.md`); criterion MET — unfloored learned-weights fused 34/37 ties vector-alone (was fused-with-multipliers 31/37) | high | reference (not a change) | remem-recall |
| Embedder thread sweep: Rayon default (all cores) optimal, 1->10.9s .. 8->1.7s per op; no code change, constrain via `RAYON_NUM_THREADS` only (`bench_threads.rs`) | low | done (no change needed) | remem-embed |
| CUDA build blocked upstream (candle-kernels `__hmax_nan` vs CUDA 13.x sm_75) | medium | open (PR huggingface/candle#3909 unmerged, tracker #5; CUDA 12.6 side-by-side + fork-patch workarounds in `docs/cuda.md`) | remem-embed |
| Near-duplicate report on write: landed and committed (knn k=3, `SIMILAR_MAX_DISTANCE` 0.48 calibrated over 780 fixture pairs via `near-dup-calibrate.rs`; fires on 3/4 paraphrases, silent on all distinct pairs; CLI `similar:` line + MCP `similar` array) | medium | done (#2 closed) | remem-recall |
| CLI forget/related/central/path | low | done (#3 closed; full CLI/MCP parity `f40b529`, `docs/cli-mcp-parity.md`) | remem-recall |
| MCP recall `since`/`until` | low | done (landed in `crates/remem-mcp/src/main.rs`) | remem-mcp |
| Recall `k=0` (returned 5 hits, now returns `[]`; `5ab9995` + cli/engine tests) | low | done | remem-recall |
| CLI `list --limit` (`aed76fb` + CLI test) | low | done | remem-recall |
| Year-bound checked dates (reject out-of-range years; `66b712e` + glossary) | low | done | remem-recall |
| Reserved hub-prefix rejection (`hub:`/`hubs:`/`hubs/`; `800c668` + graph tests) | medium | done | remem-graph |
| CLI/store path hardening (HOME-unset + tilde errors name HOME, `cc27818`) | medium | done | remem-recall |
| Abstention guard + adversarial battery: content-free queries return `[]` (reason `query-empty`; `b95af36`); battery 4/10 -> 6/10 (`docs/adversarial-battery.md` post-guard re-run `2350334`; #4/#9 flipped, other eight top-1 ids unchanged) | medium | done | remem-recall |
| Token-lean skill (`skills/remem/SKILL.md`, 2849 chars / 596 tokens, troubleshooting section `fbdf06c`: disk-IO-778 fix, Cpu-stderr note, agent/session hard filter, `--json` score/reasons, skip rebuild if `./target/debug/remem` runs) | low | done | remem-recall |
| Tag-anchor ranking (Q27 supervision): arm-discount in recall (predicted 37/37 @1 per `docs/rank1-roadmap.md`) | medium | REVERTED (`5f1840f`): live 32/37 @1 vs 35/37 baseline — Q27 rank 3 -> 1, Q28 2 -> 3, Q3/Q16/Q29/Q33 1 -> 2; @5 37/37, adv 3/3 held. Tracker #6 stays open (Q27 @1 unsupervised); hybrid `e3f1da9` 35/37 @1 is the standing config | remem-recall |
| MCP nits: case-sensitive `Content-Length` header, `related` vs `link` unknown-id consistency | low | open | remem-mcp |
| CI green at HEAD (run 34326311715 stemmer-analysis docs + 4 recent doc commits: all success on v3; earlier fmt-red pair 34301206759/34300862327 long superseded) | low | done | remem-recall |
| Release checks (`docs/release-check-2.md` all green on release binaries; check 3 in flight) | low | in-flight (release sibling owns; this file does not duplicate it) | remem-recall |
| Stemmer analysis (`docs/stemmer-analysis.md`, `b62f02f` docs-only +115): porter divergences mapped (auditor/audit = Q28, decid/decis = Q27, plus 4 latent pairs); drop-porter (32/37 @1, breaks Q8) and trigram (31/37 @1, Q39 junk) rejected; hybrid recommendation (keep porter, query-side 4-char prefix arm, gate len>=5) | high | done | remem-recall |
| Hybrid prefix arm in `fts_quote` (query text only, no schema/index rebuild; surviving tokens len>=5 get a 4-char prefix arm) | high | done (`e3f1da9`; prediction met: 35/37 @1, 37/37 @5, adv 3/3 — see `docs/eval.md` hybrid section) | remem-store |
| Store tests pass without REMEM_DB redirect (`env -u REMEM_DB cargo test -p remem-store`: 22 passed) | low | done | remem-store |
| Janitor disk reclaim (1.4G reported by store sibling; not re-measured in this refresh) | low | in-flight (store sibling owns) | remem-store |

## Notes

- NOW: arm-discount REVERTED (`5f1840f`), no discount code in `crates/` at HEAD. Post-mortem in `docs/rank1-roadmap.md`: offline rerank on `--k 40` lists did not transfer to live `--k 5`; lesson — always confirm with `scripts/eval.sh` on a fresh DB before writing a prediction. Holds: hybrid `e3f1da9` 35/37 @1, 37/37 @5, adv 3/3; battery 6/10.
- Abstention guard: stopword-only and content-free queries return `[]` with reason `query-empty` (`b95af36`); battery re-run `2350334` flipped #4/#9 to Y, other eight top-1 ids unchanged.
- Multiplier question LANDED (`aebe0c2`): RRF k=30, FTS 1.0 / vec 0.5 / graph 1.0, importance out of the formula (stored attribute only), recency 0.9+0.1x, tag gate 2.0x. Docs do not retune constants.
- Temporal verdict: keep the narrowed recency band; use `--since/--until` windows when the caller knows the date; future-date clamp to 1.0 is a flagged design question, not a measurement artifact.
- Tags now score in FTS: 2-column fts5 (content 1.0, tags 2.0) with old-DB migration; tag-only anchors get lexical matches plus the 1.05x boost — but 1.05x vs 1.2x measure the same recall, the constant is not the lever, the bands are.
- Stemmer verdict (`docs/stemmer-analysis.md`): keep porter, add query-side 4-char prefix arm (len>=5); LANDED, recovered both floor-dropped stem-gap anchors, fused 37/37 @5 with adv unchanged.
- Importance validation: `clamp_importance` (0..1, NaN -> 0.5) enforced on insert/get/update; MCP `importance` strings error loudly. Learned-weights build takes importance out of the scoring formula.
- Small-query fixes landed: `k=0` returns `[]` (`5ab9995`); `list --limit` bounds newest-first output (`aed76fb`); year-bound dates reject out-of-range years (`66b712e`); CLI graph cmds forget/related/central/path close the MCP gap (`f40b529`, #3 closed).
- CLI help: every flag now has doc text (`5c5200e`); skill `skills/remem/SKILL.md` is 2849 chars with troubleshooting (`fbdf06c`), still token-lean.
- MCP surface: 11 tools over stdio; release check 2 all green on release binaries (`docs/release-check-2.md`, release sibling owns). Live db `~/.remem/remem.db`: 36 memories, 41 nodes, 47 edges.
- CI: run 34326311715 (stemmer-analysis docs) plus 4 recent doc commits all success on v3. Earlier fmt-red pair superseded.
- Issues: #1 (floor 0.02 opt-in) / #2 (near-dup 0.48) / #3 (CLI graph cmds) / #4 (MCP occurredAt) closed; only #5 (CUDA upstream) and #6 (tag-anchor supervision — hybrid landed, Q27 @1 remains) open; the open rows above reference them.
- Discarded per inspiration.md: cross-encoder rerank, LLM entity extraction, dual-level keyword modes.

## Verification (this refresh)

- Help texts: `git show 5c5200e` touches only `crates/remem-recall/src/main.rs` (+30/-3 doc comments); covers remember/recall/list/forget/purge/link/related/central/path/stats/validate, no behavior change.
- Skill: `wc -c skills/remem/SKILL.md` reads 2849 (`fbdf06c` +8 lines); troubleshooting lists disk-IO-778 fix, Cpu-stderr note, agent/session filter, `--json` scores, skip rebuild.
- Battery: `docs/adversarial-battery.md` live-DB read-only rerun 6/10 post-guard (`2350334`, up from 4/10); #4/#9 flipped, other eight top-1 ids unchanged; `b95af36` returns `[]` with `query-empty`.
- k=0: `5ab9995` returns `[]` instead of 5 hits; cli/engine tests cover it.
- List limit: `aed76fb` adds `--limit` with CLI test; newest-first, unbounded default.
- Year-bound dates: `66b712e` rejects out-of-range years; glossary updated.
- CLI graph cmds: `f40b529` adds forget/related/central/path; closes #3, parity map updated.
- MCP occurredAt: `6f9d0b3` passes unix-secs or YYYY-MM-DD through; closes #4.
- Near-dup: knn k=3, `SIMILAR_MAX_DISTANCE` 0.48 over 780 pairs; 3/4 paraphrases fire.
- Floor: #1 closed as opt-in; floor-first `5733447` (rank -> truncate -> floor -> boost -> resort -> pack) judges unboosted scores, Q39 junk can no longer 2.0x past the floor.
- Stemmer: `b62f02f` is docs-only (+115 `docs/stemmer-analysis.md`); replays all 40 fixtures in scratch FTS5 with the real `fts_quote` logic; baseline reproduces 35/37 @1, 35/37 @5 exactly.
- Stemmer divergences: auditor/audit (Q28) and decid/decis (Q27) in fixtures, 4 latent pairs (deploy/deployment, store/storage, prioritize/priority, index/indices); migrate/token families do NOT diverge.
- Stemmer rejections: drop-porter 32/37 @1 (breaks Q8), trigram 31/37 @1 + Q39 junk + short terms (`db`, `v3`, `ip`) unmatchable; porter min_length and porter+trigram tokenizer both dead ends.
- Hybrid: keep porter, query-side 4-char prefix arm for words len>=5 (`"auditors" OR "audi"*`); measured fused 35/37 @1, 37/37 @5, adv 3/3 (Q27 rank 3, Q28 rank 2), matching the prediction.
- Hybrid state: landed `e3f1da9` (store sibling, `fts_quote` + `PREFIX_MIN_LEN`/`PREFIX_LEN` + 3 new unit tests); this refresh touched docs only.
- Store tests: `env -u REMEM_DB cargo test -p remem-store` green, 22 passed, 0 failed; no redirect needed.
- Janitor: 1.4G reclaim is sibling-reported, not re-measured here (local `target/` still ~15G); row marked in-flight, store sibling owns.
- CI: `gh run list` head run 34326311715 (stemmer-analysis docs) success; next 4 doc commits all success on v3.
- Issues: `gh issue list` shows #1/#2/#3/#4 CLOSED, #5 (CUDA) and #6 (tag-anchor) OPEN.
- Learned weights: landed `aebe0c2`; live in `rank.rs` (`DEFAULT_RRF_K` 30, `TAG_MATCH_BOOST` 2.0).
- Learned weights config: FTS 1.0 / vec 0.5 / graph 1.0, importance out of `final_score`, recency 0.9+0.1x.
- Release checks: `docs/release-check-2.md` all green; check 3 in flight with release sibling.
- Fetch check: `git pull --rebase` already up to date; local v3 in sync with `origin/v3`, no merge needed.
- Dirty-tree guard: sibling weight-impl files left untouched; only this roadmap file written.
- Line budget: this file is kept at 100 lines, no emojis, docs-only change.
- Scores snapshot: hybrid `e3f1da9` 35/37 @1, 37/37 @5 floored, adversarial 3/3 (was floor-first 35/37 @5; gated 36/37 @5 2/3; learned-weights 33/37 @1); no @5 floor-drop left.
- Open design flag: future-date clamp to 1.0 stays a question, not a measurement artifact.
- Custodian: refresh verified each claim above against HEAD/worktree before marking done.
