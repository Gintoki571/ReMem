# remem v3.0 readiness — 2026-09-09 (HEAD 16617c8, worktree clean)

Verdict: NOGO (CI red x3 on fmt only; fused<vector criterion unmet; #5/#6 open).

| area | state | note |
| Tests (`cargo test --workspace`) | pass | 145 passed, 0 failed, 2 ignored |
| CI (last 3, Gintoki571/ReMem) | fail | all 3 fail at `cargo fmt --check` only (cli.rs:519-546); clippy/test/demo unreached |
| Issues | 2 open, 4 closed | open: #5 GPU upstream, #6 tag anchors; closed: #1-#4, code matches |
| demo.sh (scratch db) | pass | exit 0; MCP-vs-CLI parity, 11 tools |
| Skill (`skills/remem/SKILL.md`) | in sync | covers all 12 CLI cmds + 11 MCP tools, incl. --json/--since/--until/min-score |
| Live db (`~/.remem/remem.db`) | pass | 32 memories, 37 nodes, 39 edges; recall returns scored hits |
| Eval (docs/eval.md) | criterion unmet | fused 31/37 @1 < vector-alone 34/37; needs fused >= 34/37 |

## Blockers
1. `cargo fmt` on crates/remem-recall/tests/cli.rs (1-command code fix, out of scope for docs-only pass).
2. Fused ranking still trails best single signal (31/37 vs 34/37); post-RRF multipliers suspect.
3. #5 GPU build blocked upstream (candle-kernels vs CUDA 13.3 on sm_75); #6 tag-anchor ranking (Q27/Q28).
