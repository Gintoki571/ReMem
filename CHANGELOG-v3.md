# remem v3 changelog

## Unreleased / in flight (unmerged working-tree changes)

- Recall FTS fix: natural-language queries ignore filler words so they keep matching.
- Two clocks: recall ranks by when a memory happened, not when it was typed.
- MCP `validate` tool: reports store/graph consistency issues (empty means healthy).

## Engine (store/graph/embed/recall/CLI)

- Rust memory engine: SQLite store with vector search, graph links, embeddings, and recall CLI.
- `validate` command: checks store/graph consistency and reports issues.
- Recall ranking tuned for recency and importance weighting.

## MCP server

- MCP server over stdio: remember/recall/list/link/events/forget/stats/validate.
- Read-only annotations and selectivity hints so agents pick sharper tools.

## Quality (eval/validate/fixes)

- Recall eval harness: 16% @1 / 64% @5 on the probe set, with notes.
- Release performance numbers published.

## Docs

- README-v3: Rust workspace guide, CUDA notes, and agent skill.
- AGENTS.md contract and remem agent skill.
- Prime-agent MCP registration docs.

## CI

- Rust toolchain via dtolnay action for reliable CI builds.
