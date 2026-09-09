# docs index

## Engine
- architecture-v3.md — v3 engine design (store+recall+graph); read first for code changes.
- glossary.md — term definitions (RRF, floor, kinds); all readers.
- demo.md — onboarding core-loop walkthrough on scratch DB; new users.
- prime-agent-integration.md — MCP stdio wiring into prime-agent; integrators.
- cli-mcp-parity.md — CLI vs MCP command coverage; CLI/MCP contributors.
- graph-review-2.md — graph lib review (neighbors, dangling edges); graph contributors.
- migrate-gap.md — content_hash backfill gap; store maintainers.
- inspiration.md — hindsight/LightRAG ideas mined into v3; designers.

## Quality
- eval.md — 40-fixture recall-quality eval; quality readers.
- ranking-study.md — FTS vs vector vs fused comparison; rank tuners.
- floor-decision.md — --min-score floor decision (issue #1); rank tuners.
- temporal-eval.md — recency-weight temporal study; rank tuners.
- tag-supervision.md — tag-query autopsy Q27/Q28 (issue #6); rank tuners.
- tagboost-review.md — TAG_MATCH_BOOST review verdict REVISE; reviewers.
- multiplier-proposal.md — post-RRF multiplier proposal; rank tuners.
- weight-spike.md — offline fusion-weight grid search; rank tuners.
- landing-weights.md — learned recall weights landing spec; implementers.
- adversarial-battery.md — adversarial recall battery results; testers.
- quality-roadmap.md — quality work plan from eval+ranks; planners.
- fuzz-report.md — CLI fuzz report (debug binary); testers.
- mcp-fuzz-report.md — MCP stdio fuzz report; MCP testers.
- concurrency-recheck.md — busy_timeout contention recheck; store reviewers.

## Operations
- ops.md — ops guide (schema, backup, contention); operators.
- perf.md — release-binary perf numbers; operators.
- latency-amortized.md — MCP persistent vs CLI spawn latency; operators.
- cuda.md — GPU embedding status (broken, CPU default); GPU users.
- cuda-unblock.md — candle-kernels upstream block status; GPU watchers.
- security-review.md — workspace security review; reviewers.
- dependency-audit.md — cargo tree + OSV audit; maintainers.
- mcp-soak.md — MCP server soak test; MCP operators.
- provider-fallbacks.md — RLM delegation fallback candidates; operators.
- dogfood-digest.md — self-hosted memory digest (26 memories); all readers.

## Process
- release-check.md — release parity check (rev+bytes); releasers.
- release-check-2.md — second release check (build+rev); releasers.
- readiness.md — v3.0 readiness verdict NOGO; releasers.
- RELEASE-v3.0.md — v3.0 release notes DRAFT; releasers.
- landing-checklist.md — qwen CLI + glm MCP landing list; implementers.
- completion-summary.md — what v3 built (6 crates); newcomers.
- goal-audit.md — thread-goal audit vs v3 scope; leads.

## Planning
- merge-v3-to-main.md — v3-to-main merge recon; leads.
- merge-watch.md — worktree-vs-HEAD merge watch; leads.
