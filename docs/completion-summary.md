# remem v3 -- completion summary

## What was built
- 6 crates: remem-types, remem-store, remem-embed, remem-recall, remem-graph, remem-mcp (+ `remem` CLI binary).
- One-file stack: single SQLite DB (FTS5 + sqlite-vec 0.1.9 + Graphite Cypher nodes/edges tables).
- Local embeddings: candle (MiniLM-class) in-process, CPU default, CUDA feature blocked upstream (see #5).
- Recall pipeline: FTS + vector RRF fusion (k=30, FTS 1.0 / vector 0.5 / graph 1.0), gated tag boost, min-score floor (default off).
- MCP server + CLI at parity: 11 tools (remember/recall/list/link/forget/purge/stats/validate/related/central/path); occurredAt passthrough fixed (#4).

## Final numbers (verified 2026-09-09)
- Tests: 161 passed, 0 failed, 2 ignored (`cargo test --workspace`).
- Recall (40 fixtures, 37 answerable + 3 adversarial, floored k=5, min-score 0.017): @1 35/37, @5 36/37.
- Adversarial: 2/3 pass (regressed from 3/3; gated 2.0x tag boost lifts one junk hit over the floor).
- CI: success on v3, run 34317094557 (`docs(v3): changelog delta 4`, 2026-09-09).
- Issues: #1-#4 closed; #5 (GPU/candle-kernels vs CUDA 13.3 on sm_75) and #6 (tag-anchor Q27/Q28 supervision) open.
- Live DB (`~/.remem/remem.db`): 34 memories, 39 nodes, 43 edges.
- MCP: 11 tools at `tools/list` (remember/recall/list/link/forget/purge/stats/validate/related/central/path).
- Skill (`skills/remem/SKILL.md`): 2849 chars.

## How it was built
- TDD: failing eval/probe first, fix second, full suite + `scripts/eval.sh` re-run to close.
- 3-model crew + overflow lane: parallel implementation lanes with a merge/verify lane.
- Verifier pattern: independent re-run of every claimed number before it enters docs.
- Dogfood loop: the agent used remem itself (34 live memories); misses became fixtures (Q27/Q28, junk queries).

## What remains (for the human)
- Adversarial 2/3: floor re-measurement at gated weights or a narrower gate (context under #6).
- #5 upstream: candle-kernels vs CUDA 13.3 on sm_75 blocks the GPU build; CPU is the supported path.
- #6 supervision: Q27/Q28 tag-only anchors need real supervision signal beyond the 1.05x/2.0x gate.
- Merge decision: v3 -> main is a human call; see `docs/merge-v3-to-main.md`. Gate: 35/37 @1 held, adv 3/3 or accepted 2/3, #5/#6 triaged.
