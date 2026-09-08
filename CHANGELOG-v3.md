# remem v3 changelog

## Unreleased / in flight (unmerged working-tree changes)

- Graph algos in flight: `Graph::central` (PageRank over memory nodes) and `Graph::shortest_path` (directed, empty when unreachable).
- Score floor in flight: `apply_floor` drops recall hits below a minimum score (0.0 means off).

## Engine (store/graph/embed/recall/CLI)

- Rust memory engine: SQLite store with vector search, graph links, embeddings, and recall CLI.
- `validate` command: checks store/graph consistency and reports issues.
- Recall ranking tuned for recency and importance weighting.
- FTS stopword fix, importance rescale, and second-resolution event clock for ranking.
- Content-hash dedup on write: same kind and content returns the existing id.
- Hard purge in the store: removes the row, FTS entry, and embedding.
- Token-budget packing (`--max-chars`, top hit never dropped) and a `purge` CLI command.

## MCP server

- MCP server over stdio: remember/recall/list/link/events/forget/stats/validate.
- Read-only annotations and selectivity hints so agents pick sharper tools.
- `validate` and `forget` tools (empty validate result means healthy).
- `purge` tool: hard-deletes a forgotten memory.
- `related` tool: graph neighbors of a memory id, with optional edge-type filter.
- `recall` accepts `maxChars`, passed through to token-budget packing.

## Quality (eval/validate/fixes)

- Recall eval harness: 16% @1 / 64% @5 on the probe set, with notes.
- Release performance numbers published.
- Eval fixtures grown from 25 to 40.
- 40-fixture eval: 54% @1 / 97% @5.
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
