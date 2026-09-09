# MCP server soak test

## Setup

- Branch: v3 (testing only, no source changes).
- Binary: debug `target/debug/remem-mcp` (`cargo build -p remem-mcp` green, no retry needed).
- DB: scratch `REMEM_DB=/tmp/remem-soak.db`, removed before start.
- Session: one server process, single stdio session, line-delimited JSON-RPC.
- Workload: 300 sequential calls, each with a 10 s timeout.
  - 1x `initialize`, 1x `tools/list`.
  - 60x `tools/call remember` (kinds cycled fact/decision/mistake/preference/event/note, tagged `soak`).
  - 238x mixed `tools/call`: recall (query "soak caching retries", k=5), list (limit 10),
    link (chained remember ids, rel "relates"), stats, validate — round-robin.
- Driver: `/tmp/soak.py` (kept outside the repo). RSS via `ps -o rss= -p <pid>` at start/end.

## Results

| Metric | Value |
|---|---|
| Completed responses | 300 / 300 |
| JSON-RPC / tool errors | 0 |
| Hangs (10 s timeout) | 0 |
| Server alive at end | yes (terminated after RSS sample) |
| RSS start | 452104 KB (~441.5 MB, includes embedding model load) |
| RSS end | 461832 KB (~451.0 MB) |
| RSS growth | +9728 KB (~9.5 MB) |
| remember ids collected | 60 / 60 |

Post-kill CLI check on the same DB (`REMEM_DB=/tmp/remem-soak.db`):

| Check | Result |
|---|---|
| `remem validate` | exit 0, no issues |
| `remem stats` | 60 memories, 60 graph nodes, 30 edges |
| `remem recall "soak caching retries" -k 3` | returns soak hits with scores |
| `remem list` | all 60 soak memories present, newest first |

## Verdict

Stable: Y. 300/300 calls answered with no errors or hangs, RSS growth ~9.5 MB over the run,
and post-kill `validate` plus spot recall confirm the data is intact.
