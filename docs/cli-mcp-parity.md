# CLI / MCP parity (v3 branch)

Source: `crates/remem-recall/src/main.rs` (`Cmd` enum + dispatch) vs
`crates/remem-mcp/src/main.rs` (`tools_list` + `dispatch`).
DB: CLI `--db` (global, `REMEM_DB` env, default `~/.remem/remem.db`);
MCP `REMEM_DB` env only, no flag.

| Operation | CLI | MCP | Gap |
|---|---|---|---|
| remember | `remember <kind> <text...>` + `--tags`, `--agent`, `--session`, `--importance` (default 0.5), `--occurred-at` | `remember {kind, content, tags, agent, session, importance, occurredAt}` | Parity. Arg name differs (`text` joined vs `content`). Output differs: CLI prints bare id (+ `similar:` line); MCP returns `{"id", "similar": [{id, distance}]}`. |
| recall | `recall <query...>` + `--k` (default 5), `--json`, `--agent`, `--session`, `--since`, `--until`, `--max-chars`, `--min-score` (default 0.0 = off) | `recall {query, k, agent, session, maxChars, minScore, since, until}` | Parity on filters. CLI `--json` is formatting only (MCP always JSON). MCP clamps `k` to 1000, CLI does not. MCP JSON hits include `agent`/`session`; CLI `--json` hits omit them. |
| list | `list [--json]` (newest first) | `list {limit?}` (clamped to 1000) | Each lacks one flag: CLI has no `limit`, MCP has no `--json` (always JSON). |
| link | `link <from> <to> [--rel]` (silent) | `link {from, to, rel?}` (returns `{"ok": true}`) | Parity; output differs only. |
| forget | MISSING | `forget {id}` (soft delete, returns `{"forgotten": bool}`) | CLI has no soft delete. |
| purge | `purge <id>` (hard delete, errors on unknown id) | `purge {id}` (returns `{"purged": bool}`) | Parity; output contract differs (exit-error vs boolean). |
| stats | `stats` (pretty JSON) | `stats {}` | Parity. |
| validate | `validate` (prints lines, exit 1 if any) | `validate {}` (returns `{"issues": [...]}`) | Parity; output differs only. |
| related | MISSING | `related {id, rel?}` (`[]` on unknown id) | CLI has no neighbor read. |
| central | MISSING | `central {limit?}` (default 10, clamped to 1000) | CLI has no PageRank read. |
| path | MISSING | `path {from, to}` (`[]` if unreachable) | CLI has no shortest-path read. |

## Gaps

CLI-missing (MCP-only), 4: `forget`, `related`, `central`, `path`.
MCP-missing (CLI-only), 2: `list --limit` vs `list limit` (functional),
`--json` (formatting only, no functional gap).

## Fill order (smallest first)

1. CLI `list --limit N` (truncate after fetch, mirrors MCP `limit`).
2. CLI `forget <id>` (wrapper over engine `forget`, soft delete before `purge`).
3. CLI `recall --json` should include `agent`/`session` (match MCP hit shape).
4. CLI `related <id> [--rel]`, `central [--limit]`, `path <from> <to>` (read-only printers over engine `graph()` methods).
