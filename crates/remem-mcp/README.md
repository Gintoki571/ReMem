# remem-mcp — MCP stdio server for the ReMem v3 engine

## Transport verdict: hand-rolled JSON-RPC stdio, NOT `rmcp`

`cargo search rmcp` shows the crate at **3.x** (not 0.x) with a heavy async
tree (tokio, schemars, transport layers) and a churn-prone macro API.
MCP stdio is just newline-delimited JSON-RPC 2.0 (`initialize`,
`tools/list`, `tools/call`), so this server implements that directly in
~150 lines of sync std with `serde_json` only — zero new dependencies,
green today. Also accepts `Content-Length`-framed messages.
Revisit `rmcp` if Streamable HTTP or sampling/progress is ever needed.

## Run

`REMEM_DB` env selects the SQLite file (default `~/.remem/remem.db`).
Same engine wiring as the `remem` CLI: real local embedder with
`StubEmbedder` fallback. Logs go to stderr; stdout is pure JSON-RPC.

## Tools

`remember(kind, content, tags?, agent?, session?, importance?, occurredAt?)` (occurredAt: unix seconds or `YYYY-MM-DD`),
`recall(query, k?, agent?, session?, maxChars?, minScore?)`, `list(limit?)`,
`link(from, to, rel?)`, `related(id, rel?)`, `central(limit?)`, `path(from, to)`,
`forget(id)`, `purge(id)`, `stats`, `validate`.
