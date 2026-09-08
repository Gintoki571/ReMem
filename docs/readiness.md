# remem v3.0 readiness — 2026-09-08 (HEAD b24d548, worktree dirty, read-only check)

Verdict: NOGO (CI red x3; fused<vector criterion unmet; #1/#2/#3/#6 open).

| area | state | blocker or - |
| Tests (`cargo test --workspace`, current tree) | pass | - (135 passed, 0 failed, 2 ignored) |
| CI (last 3, Gintoki571/ReMem) | fail | all completed failure: release-check-3, parity-refresh, mcp-since/until |
| Docs (RELEASE-v3.0.md) | stale | see stale lines below |
| Issues #1-6 vs code | 1 open, 4 closed-OK | #1/#2/#3/#5/#6 open; #4 closed, code matches |
| Dogfood (live db + recall smoke) | pass | - (26 memories; `recall --k 3` returns 3 scored hits) |
| Worktree | dirty | 8 modified + untracked recall/examples (sibling near-dup WIP) |

## Stale lines in docs/RELEASE-v3.0.md
- Quality/Eval: HEAD ref 7147dc3 vs current b24d548; counts un-reverified since.
- Quality/Tests: claims 126 passed + 1 failed (`cli_recall_max_chars`) + per-crate table; now 135/0/2, cli 7/7 green.
- Quality/Tests: claims plain `cargo test` does not compile (knn-probe.rs type error); it compiles now. Examples dir also has near-dup-calibrate.rs (undocumented).
- Known gaps: "MCP recall since/until also CLI-only" is wrong; MCP since/until landed (a3111f1, verified in mcp/main.rs).
- Known gaps: "#4 closable" is closed; "1.05x prefix boost" vs issue #6 title 1.2x.

## Blockers
- Fused 31/37 @1 < vector-alone 34/37 (criterion unmet); floor default undecided (#1); near-dup WIP uncommitted (#2); CLI gaps (#3); tag anchors Q27/Q28 (#6); GPU upstream (#5).
