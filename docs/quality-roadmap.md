# ReMem v3 quality roadmap (docs only)

Sources: `docs/eval.md` (40-fixture corrected eval + post-merge verification), `docs/ranking-study.md`, `docs/weight-spike.md`, `docs/temporal-eval.md`, `docs/multiplier-proposal.md`, `docs/tagboost-review.md`, `docs/adversarial-battery.md`, `docs/cuda-unblock.md`, `docs/cli-mcp-parity.md`, `docs/demo.md`, `docs/security-review.md`, `docs/fuzz-report.md`, `docs/mcp-fuzz-report.md`, `CHANGELOG-v3.md`.

Verified against worktree/HEAD before marking done (tags-in-FTS `a6becb3` bm25 1.0/2.0; learned weights in-flight in working tree uncommitted, code sibling owns; help texts `5c5200e`; skill 2849 chars `fbdf06c`; battery 6/10 post-guard `2350334`; CI rust fmt-red test+demo-green on runs 34301206759/34300862327; issues #1-#4 closed, #5/#6 open).

## Status table (gap | severity | status | owner)

| gap | severity | status | owner |
|---|---|---|---|
| FTS stopword-OR fix (dominant miss pattern) | high | done | remem-store |
| Tags in FTS (2-col fts5 content+tags, triggers + old-DB migration, `bm25(memories_fts, 1.0, 2.0)`; landed `a6becb3` + store tests) | medium | done | remem-store |
| Importance rescale + clamp (0..1, NaN -> 0.5, enforced on write and read) | medium | done | remem-types |
| Two-clock `occurred_at` event time (CLI `--occurred-at`) | medium | done | remem-types |
| MCP remember `occurredAt` (unix secs or YYYY-MM-DD; landed `6f9d0b3`; #4 closed) | medium | done | remem-mcp |
| Token-budget packing (`--max-chars`, top hit kept) | medium | done | remem-recall |
| Score floor (`apply_floor`; MCP `minScore`/`maxChars` passthrough; stays opt-in, default 0.0 = off) | medium | done (#1 closed: `--min-score 0.02` calibrated, junk 0.012-0.016 passes unfloored) | remem-recall |
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
| Tag boost for tag-only anchors: landed 1.05x, single application, case-folded, char-safe prefix (`e844cd4`) | medium | done | remem-recall |
| Post-RRF multiplier rebalance: narrowed bands `0.9 + 0.1*` landed (`e844cd4`); learned-weights adoption IN-FLIGHT (Weights default fts 1.0 / vec 0.5, importance out of formula, in working tree uncommitted — code sibling owns, do not retune here) | medium | in-flight (code sibling owns) | remem-recall |
| Weight spike (`docs/weight-spike.md`, `acfa7f4`): 729-config offline grid, winner k=30 / FTS 1.0 / vec 0.5 / multis off predicts 35/37 (+4 over 31/37 baseline; fixes Q7/Q12/Q22/Q36; Q27/Q28 still miss; 36-way tie, overfit caution, needs time-varied second set) | high | in-flight (prediction only; implementation running with code sibling) | remem-recall |
| Temporal verdict (`docs/temporal-eval.md`, `ae5bb78`): recency stays as-is (narrow band, max 11% swing, 0.49% old-vs-old); `--since/--until` windows for time-anchored queries; future-date clamp (clamps to 1.0) flagged as open design question | medium | done (docs verdict; clamp flag open) | remem-recall |
| Pure 1:1 RRF reference: 34/37 @1 (`docs/ranking-study.md`); fused-with-multipliers 31/37; bar for learned weights is fused >= 34/37 | high | reference (not a change) | remem-recall |
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
| Tag-anchor ranking beyond the 1.05x prefix boost (Q27/Q28 supervision) | low | open (tracker #6) | remem-recall |
| MCP nits: case-sensitive `Content-Length` header, `related` vs `link` unknown-id consistency | low | open | remem-mcp |
| CI fmt failure at HEAD (`5c5200e`/`fbdf06c` runs 34301206759/34300862327: rust red on `cargo fmt --check`, test+demo green) | low | open (code sibling owns the fix; docs-only refresh does not touch it) | remem-recall |
| Release checks (`docs/release-check-2.md` all green on release binaries; check 3 in flight) | low | in-flight (release sibling owns; this file does not duplicate it) | remem-recall |

## Notes

- Where fused stands: answerable 31/37 @1, 35/37 @5 floored, 37/37 @5 unfloored; adversarial 6/10 with the abstention guard (was 4/10). Single signals unchanged: FTS-alone 33/37 @1, vector-alone 34/37 @1, pure 1:1 RRF 34/37 @1.
- Abstention guard: stopword-only and content-free queries return `[]` with reason `query-empty` (`b95af36`); battery re-run `2350334` flipped #4/#9 to Y, other eight top-1 ids unchanged.
- Multiplier question IN-FLIGHT: weight-spike winner (k=30, FTS 1.0, vec 0.5, multis off) predicts 35/37; code sibling is implementing in the working tree (uncommitted lib.rs/rank.rs + cli/engine tests). Docs do not retune constants meanwhile.
- Temporal verdict: keep the narrowed recency band; use `--since/--until` windows when the caller knows the date; future-date clamp to 1.0 is a flagged design question, not a measurement artifact.
- Tags now score in FTS: 2-column fts5 (content weight 1.0, tags weight 2.0) with old-DB migration; tag-only anchors get lexical matches plus the 1.05x boost.
- Boost width: 1.05x vs 1.2x measure the same recall; the constant is not the lever, the bands are. Offline re-ranking of `--k 40` output mispredicts because list depth is 4k.
- Importance validation: `clamp_importance` (0..1, NaN -> 0.5) enforced on insert/get/update; MCP `importance` strings error loudly. Learned-weights build takes importance out of the scoring formula.
- Small-query fixes landed: `k=0` returns `[]` (`5ab9995`); `list --limit` bounds newest-first output (`aed76fb`); year-bound dates reject out-of-range years (`66b712e`); CLI graph cmds forget/related/central/path close the MCP gap (`f40b529`, #3 closed).
- CLI help: every flag now has doc text (`5c5200e`); recall keeps `--max-chars` top-hit-kept packing, `--min-score` opt-in floor, `--since/--until` windows.
- Skill: `skills/remem/SKILL.md` is 2849 chars with a troubleshooting section (`fbdf06c`); unchanged in spirit since `4844df7`, still token-lean.
- MCP surface: 11 tools over stdio; release check 2 all green on release binaries (`docs/release-check-2.md`, release sibling owns; check 3 in flight).
- CI: runs 34301206759 (`5c5200e`) and 34300862327 (`fbdf06c`) both rust-red on `cargo fmt --check`, test+demo green. Fix is a `cargo fmt` run by a code sibling.
- Issues: #1 (floor 0.02 opt-in) / #2 (near-dup 0.48) / #3 (CLI graph cmds) / #4 (MCP occurredAt) closed; only #5 (CUDA upstream) and #6 (tag-anchor supervision) open; the open rows above reference them.
- Discarded per inspiration.md: cross-encoder rerank, LLM entity extraction, dual-level keyword modes.
## Verification (this refresh)

- Help texts: `git show 5c5200e` touches only `crates/remem-recall/src/main.rs` (+30/-3 doc comments).
- Help texts cover remember: kind enum, tags, agent, session, importance, occurred-at.
- Help texts cover recall: query, k, json, agent, session, since/until windows.
- Help texts cover list: json, limit (newest-first, unbounded by default).
- Help texts cover forget/purge ids, link from/to/rel, related id/rel, central limit.
- Help texts cover path from/to, stats, validate; no behavior change in the diff.
- Skill: `wc -c skills/remem/SKILL.md` reads 2849; commit `fbdf06c` adds 8 lines.
- Skill troubleshooting lists: disk-IO-778 REMEM_DB/TMPDIR fix, Cpu-stderr-is-normal note.
- Skill troubleshooting lists: agent/session hard filter, `--json` score/reasons, skip rebuild.
- Battery: `docs/adversarial-battery.md` live-DB read-only rerun, `--k 1 --json`, 32 memories.
- Battery score 6/10 post-guard (`2350334`), up from 4/10 (`5671ec9`); #4/#9 flipped to Y.
- Abstention: `b95af36` returns `[]` with `query-empty` for content-free queries.
- k=0: `5ab9995` returns `[]` instead of 5 hits; cli/engine tests cover it.
- List limit: `aed76fb` adds `--limit` with CLI test; newest-first, unbounded default.
- Year-bound dates: `66b712e` rejects out-of-range years; glossary updated.
- CLI graph cmds: `f40b529` adds forget/related/central/path; closes #3, parity map updated.
- MCP occurredAt: `6f9d0b3` passes unix-secs or YYYY-MM-DD through; closes #4.
- Near-dup: knn k=3, `SIMILAR_MAX_DISTANCE` 0.48 over 780 pairs; 3/4 paraphrases fire.
- Floor: #1 closed as opt-in `--min-score 0.02`; junk 0.012-0.016 passes unfloored.
- CI: `gh run list --limit 2` gives runs 34301206759 (`5c5200e`) and 34300862327 (`fbdf06c`).
- CI detail: both runs rust-red (`cargo fmt --check`), test success, demo success.
- CI fix ownership: code sibling runs `cargo fmt`; this docs-only refresh does not touch it.
- Issues: `gh issue list` shows #1/#2/#3/#4 CLOSED, #5 (CUDA) and #6 (tag-anchor) OPEN.
- Learned weights: working-tree `lib.rs`/`rank.rs` + cli/engine tests uncommitted; code sibling owns.
- Learned weights default under test: FTS 1.0 / vec 0.5, importance out of the formula.
- Release checks: `docs/release-check-2.md` all green; check 3 in flight with release sibling.
- Fetch check: `git fetch origin` shows local v3 in sync with `origin/v3`; no merge needed.
- Dirty-tree guard: sibling weight-impl files left untouched; only this roadmap file written.
- Line budget: this file is kept at 100 lines, no emojis, docs-only change.
- Scores snapshot: fused 31/37 @1; spike winner predicts 35/37; bar for learned is >= 34/37.
- Open design flag: future-date clamp to 1.0 stays a question, not a measurement artifact.
- Custodian: refresh verified each claim above against HEAD/worktree before marking done.
