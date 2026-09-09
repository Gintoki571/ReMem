# remem v3.0 readiness — 2026-09-09 (HEAD 7dfe84c; sibling store edits in worktree, untouched)

Verdict: GO (code-complete; merge decision pending human).

| area | state | note |
| Tests (`cargo test --workspace`) | pass | prior: 163 passed, 0 failed, 2 ignored, 21 suites |
| CI (Gintoki571/ReMem, v3) | pass | success at HEAD 7dfe84c, run 34327182035 (fmt, clippy, test, demo) |
| Issues | 2 open, 4 closed | open: #5 GPU upstream, #6 tag anchors; closed: #1-#4 |
| Eval (docs/eval.md) | criterion MET | fused 35/37 @1 >= vector-alone 34/37; @5 35/37; adversarial 3/3 |
| demo (scripts/demo.sh, scratch db) | pass | exit 0, 12 steps; MCP-vs-CLI parity, 11 tools |
| Skill (`skills/remem/SKILL.md`) | in sync | 11 CLI cmds + 11 MCP tools, incl. --json/--since/--until/min-score |
| Live db (`~/.remem/remem.db`) | pass | 47 memories, 52 nodes, 62 edges; recall returns scored hits |
| MCP (`tools/list`) | pass | 11 tools: remember/recall/list/link/forget/purge/stats/validate/related/central/path |
| Siblings in flight | note | hybrid-impl (store) + migration + release builds (separate owners, untouched here) |

## Known gaps (open, not blockers)
1. #5 GPU build blocked upstream (candle-kernels vs CUDA 13.3 on sm_75); CPU supported.
2. #6 tag-anchor supervision follow-up (Q27/Q28 need signal beyond 1.05x/2.0x gate).
3. Recall@5 35/37: Q27 floor-dropped by design (accepted floor-first tradeoff).

## Merge precondition
- Human merge decision on docs/merge-pr-draft.md (main is direct ancestor, no conflicts).

## Verify
- `bash scripts/demo.sh` (exit 0) | `gh run list --branch v3 --limit 3` | `gh issue list --state all`
