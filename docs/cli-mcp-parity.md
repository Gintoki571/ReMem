# CLI / MCP parity (v3 branch)

Source: `crates/remem-recall/src/main.rs` (`Cmd` enum + dispatch) vs
`crates/remem-mcp/src/main.rs` (`tools_list` + `dispatch`).
DB: CLI `--db` (global, `REMEM_DB` env, default `~/.remem/remem.db`);
MCP `REMEM_DB` env only, no flag.

| Operation | CLI | MCP | Gap |
|---|---|---|---|
| remember | `remember <kind> <text...>` + `--tags`, `--agent`, `--session`, `--importance` (default 0.5), `--occurred-at` | `remember {kind, content, tags, agent, session, importance, occurredAt}` | Parity. Arg name differs (`text` joined vs `content`). Output differs: CLI prints bare id (+ `similar:` line); MCP returns `{"id", "similar": [{id, distance}]}`. |
| recall | `recall <query...>` + `--k` (default 5), `--json`, `--agent`, `--session`, `--since`, `--until`, `--max-chars`, `--min-score` (default 0.0 = off) | `recall {query, k, agent, session, maxChars, minScore, since, until}` | Parity on filters. CLI `--json` is formatting only (MCP always JSON). MCP clamps `k` to 1000, CLI does not. MCP JSON hits include `agent`/`session`; CLI `--json` hits omit them. |
| list | `list [--json] [--limit N]` (newest first) | `list {limit?}` (clamped to 1000) | Parity on limit; MCP has no `--json` (always JSON, formatting only). |
| link | `link <from> <to> [--rel]` (silent) | `link {from, to, rel?}` (returns `{"ok": true}`) | Parity; output differs only. |
| forget | `forget <id>` (soft delete, row kept, graph node and edges dropped) | `forget {id}` (soft delete, returns `{"forgotten": bool}`) | Parity; output differs only. |
| purge | `purge <id>` (hard delete, errors on unknown id) | `purge {id}` (returns `{"purged": bool}`) | Parity; output contract differs (exit-error vs boolean). |
| stats | `stats` (pretty JSON) | `stats {}` | Parity. |
| validate | `validate` (prints lines, exit 1 if any) | `validate {}` (returns `{"issues": [...]}`) | Parity; output differs only. |
| related | `related <id> [--rel]` (neighbours as `id  rel`, Memory-only, sorted) | `related {id, rel?}` (`[]` on unknown id) | Parity; output differs only. |
| central | `central [--limit]` (default 10, PageRank as `id  score`, desc) | `central {limit?}` (default 10, clamped to 1000) | Parity; output differs only. |
| path | `path <from> <to>` (shortest path, empty if unreachable) | `path {from, to}` (`[]` if unreachable) | Parity; output differs only. |

## Gaps

CLI-missing (MCP-only), 0: `forget`, `related`, `central`, `path` all landed on the CLI (tracker #3 closed).
MCP-missing (CLI-only), 1: `--json` (formatting only, no functional gap).

## Fill order (smallest first)

1. DONE: CLI `list --limit N` landed (mirrors MCP `limit`).
2. DONE: CLI `forget <id>` landed (tracker #3 closed).
3. CLI `recall --json` should include `agent`/`session` (match MCP hit shape).
4. DONE: CLI `related` / `central` / `path` landed (tracker #3 closed).
