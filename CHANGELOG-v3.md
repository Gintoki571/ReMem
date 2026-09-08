# remem v3 changelog

## Unreleased / in flight (unmerged working-tree changes)

- Graph algos landed: `Graph::central` (PageRank over memory nodes) and `Graph::shortest_path` (directed, empty when unreachable), exposed as CLI + MCP `central`/`path`.
- Score floor landed: `apply_floor` drops recall hits below a minimum score (default 0.0 = off, opt-in `--min-score 0.02`; floor decision tracker #1 closed).

## Engine (store/graph/embed/recall/CLI)

- Rust memory engine: SQLite store with vector search, graph links, embeddings, and recall CLI.
- `validate` command: checks store/graph consistency and reports issues.
- Recall ranking tuned for recency and importance weighting.
- FTS stopword fix, importance rescale, and second-resolution event clock for ranking.
- Content-hash dedup on write: same kind and content returns the existing id.
- Hard purge in the store: removes the row, FTS entry, and embedding.
- Token-budget packing (`--max-chars`, top hit never dropped) and a `purge` CLI command.

## MCP server

- MCP server over stdio: 11 tools (remember/recall/list/link/forget/purge/stats/validate/related/central/path).
- Read-only annotations and selectivity hints so agents pick sharper tools.
- `validate` and `forget` tools (empty validate result means healthy).
- `purge` tool: hard-deletes a forgotten memory.
- `related` tool: graph neighbors of a memory id, with optional edge-type filter.
- `recall` accepts `maxChars`, passed through to token-budget packing.

## Quality (eval/validate/fixes)

- Recall eval harness: 16% @1 / 64% @5 on the probe set, with notes.
- Release performance numbers published.
- Eval fixtures grown from 25 to 40.
- 40-fixture eval: fused recall@1 31/37 (84%), recall@5 37/37 unfloored (35/37 floored at `--min-score 0.02`); single signals 33/37 FTS-alone, 34/37 vector-alone.
- Release parity check: binary sizes, MCP tool count, and a remember/recall roundtrip.

## Docs

- README-v3: Rust workspace guide, CUDA notes, and agent skill.
- AGENTS.md contract and remem agent skill.
- Prime-agent MCP registration docs.
- Provider fallbacks guide.
- CLI flags and MCP tools refresh.
- Security review notes.

## CI

- Rust toolchain via dtolnay action for reliable CI builds.
