# CLI / MCP parity (v3 branch, read-only recon)

Source: `crates/remem-recall/src/main.rs` (`Cmd` enum + dispatch) vs `crates/remem-mcp/src/main.rs` (`tools_list` + `dispatch`).
DB selection: CLI `--db` (global, `REMEM_DB` env, default `~/.remem/remem.db`); MCP `REMEM_DB` env only, no flag.

| Operation | CLI status | MCP status | Notes / exact flag-arg gap |
|---|---|---|---|
| remember | `remember <kind> <text...>` + `--tags`, `--agent`, `--session`, `--importance`, `--occurred-at` | `remember {kind, content, tags, agent, session, importance}` | MCP missing `occurred_at` (CLI `--occurred-at`). Arg name differs: CLI `text` (variadic, joined) vs MCP `content`. |
| recall | `recall <query...>` + `--k`, `--json`, `--agent`, `--session`, `--since`, `--until`, `--max-chars`, `--min-score` | `recall {query, k, agent, session, maxChars, minScore}` | MCP missing `since`/`until` (CLI `--since`, `--until`, unix-seconds or YYYY-MM-DD). Otherwise parity: `--k`/`k` (default 5), `--max-chars`/`maxChars`, `--min-score`/`minScore`. CLI `--json` is output formatting only (MCP always returns JSON). MCP clamps `k` to `MAX_TOP_K=1000`; CLI does not clamp. Default floor differs: CLI `--min-score` defaults to `DEFAULT_MIN_SCORE` (currently 0.0); MCP defaults to no floor unless `minScore` passed. |
| list | `list [--json]` (full list, newest first) | `list {limit?}` (JSON array, `limit` clamped to `MAX_TOP_K=1000`) | CLI missing `limit` (MCP `limit`). MCP has no `--json` flag because output is always JSON. |
| link | `link <from> <to> [--rel]` | `link {from, to, rel?}` | Parity. Both require both endpoints; `rel` optional on both. |
| forget (soft delete) | MISSING | `forget {id}` (required) | CLI has no `forget`; only `purge`. MCP `forget` returns `{"forgotten": bool}`. |
| purge (hard delete) | `purge <id>` (store purge + graph `forget`, errors on unknown id, prints `purged {id}`) | `purge {id}` (required, returns `{"purged": bool}`) | Operation parity; output contract differs (exit-error vs boolean). MCP `purge` skips graph step when nothing was purged; CLI errors first. |
| stats | `stats` (pretty JSON) | `stats {}` | Parity. |
| validate | `validate` (prints lines, exit 1 if any) | `validate {}` (returns `{"issues": [...]}`) | Parity; output contract differs only. |
| related | MISSING | `related {id, rel?}` | CLI has no graph-neighbor read. Unknown id returns `[]`, optional `rel` filter. |
| central | MISSING | `central {limit?}` (default 10, clamped to `MAX_TOP_K=1000`) | CLI has no PageRank read. |
| path | MISSING | `path {from, to}` (returns `[from, .., to]`, `[]` if unreachable) | CLI has no shortest-path read. |

## Gaps

CLI-missing (MCP-only), 4: `forget {id}`, `related {id, rel?}`, `central {limit?}`, `path {from, to}`.
MCP-missing (CLI-only), 4: `remember --occurred-at`, `recall --since`, `recall --until`, `list --json` (formatting-only; MCP always JSON, so no functional gap). Functional MCP-missing count excluding formatting: 3. Asymmetric flag within a shared op: `list limit` (MCP-only) vs `--json` (CLI-only).

## Recommended fill order (smallest gaps first)

1. Add MCP `occurredAt` to `remember` (one optional string, same `parse_time` path as CLI `--occurred-at`).
2. Add MCP `since`/`until` to `recall` (two optional strings, reuse CLI `parse_time`).
3. Add CLI `list --limit N` (truncate after fetch, mirrors MCP `limit` semantics).
4. Add CLI `forget <id>` (thin wrapper over engine `forget`, matches MCP soft-delete before hard `purge`).
5. Add CLI `related <id> [--rel]`, `central [--limit]`, `path <from> <to>` (read-only graph printers over existing engine `graph()` methods; largest but purely additive).
