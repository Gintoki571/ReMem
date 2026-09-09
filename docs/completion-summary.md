# remem v3 -- completion summary

## What was built
- 6 crates: remem-types, remem-store, remem-embed, remem-recall, remem-graph, remem-mcp (+ `remem` CLI binary).
- One-file stack: single SQLite DB (FTS5 + sqlite-vec 0.1.9 + Graphite Cypher nodes/edges tables).
- Local embeddings: candle (MiniLM-class) in-process, CPU default, CUDA feature blocked upstream (see #5).
- Recall pipeline: FTS + vector RRF fusion, tag gate, importance/recency weighting, min-score floor (default off).
- MCP server + CLI at parity: remember/recall/forget/related/central/path (occurredAt passthrough fixed, #4).

## Final numbers (verified 2026-09-09)
- Tests: 159 passed, 0 failed (`cargo test --workspace`).
- Recall (40 fixtures, 37 answerable + 3 adversarial, floored k=5, min-score 0.017): @1 35/37, @5 36/37.
- Adversarial: 2/3 pass (regressed from 3/3; gated 2.0x tag boost lifts one junk hit over the floor).
- CI: green on v3, run 34309309280 (`docs(v3): goal audit, all green`).
- Issues: #1-#4 closed, #5 (GPU/candle-kernels vs CUDA 13.3) and #6 (tag-anchor Q27/Q28 supervision) open.
- Live DB (`~/.remem/remem.db`): 32 memories.

## How it was built
- TDD: failing eval/probe first, fix second, full suite + `scripts/eval.sh` re-run to close.
- 3-model crew + overflow lane: parallel implementation lanes with a merge/verify lane.
- Verifier pattern: independent re-run of every claimed number before it enters docs.
- Dogfood loop: the agent used remem itself (32 live memories); misses became fixtures (Q27/Q28, junk queries).

## What remains (for the human)
- Adversarial 2/3: floor re-measurement at gated weights or a narrower gate (context under #6).
- #5 upstream: candle-kernels vs CUDA 13.3 on sm_75 blocks the GPU build; CPU is the supported path.
- #6 supervision: Q27/Q28 tag-only anchors need real supervision signal beyond the 1.05x/2.0x gate.
- Merge decision: v3 -> main is a human call; see `docs/merge-v3-to-main.md`. Gate: 35/37 @1 held, adv 3/3 or accepted 2/3, #5/#6 triaged.
