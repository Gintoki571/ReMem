# Release Check 2

Date: 2026-09-08 (build timestamp 23:42 local)
Branch: v3
Git rev: 1db31b0e2165c71628d44aab8befd92a26076512 (pull: already up to date; HEAD commit ef1e7452 "docs(v3): release notes draft")
Source changes: none (uncommitted local edits by sibling session left untouched, not built... actually built tree includes them - see note)

## Build

- `cargo build --release -p remem-recall -p remem-mcp`: PASS (7.8s, clean)
- Binary sizes:
  - `target/release/remem` (remem-recall CLI): 13,463,688 bytes (12.8 MiB)
  - `target/release/remem-mcp`: 12,943,256 bytes (12.3 MiB)

## Smoke results (scratch db /tmp/remem-rel2.db via RELEASE binaries)

| Command | Result | Notes |
|---|---|---|
| remember (CLI) | PASS | returned UUID + "embedder: Cpu (768d)"; x2 memories |
| recall (CLI) | PASS | --json ranked hits with fts/vector/recent reasons |
| link (CLI) | PASS | edge created; stats showed 2 nodes / 1 edge |
| validate (CLI) | PASS | exit 0 pre-purge; exit 1 with orphan issue after purge (expected) |
| stats (CLI) | PASS | correct JSON counts before/after purge |
| purge (CLI) | PASS | "purged <id>"; stats dropped to 1 memory / 0 edges |
| forget (MCP tool) | PASS | {"forgotten":true}; stats + list confirm removal |
| MCP tools/list | PASS | 11 tools over stdio: remember, recall, list, link, forget, purge, stats, validate, related, central, path |
| MCP remember/list/stats/validate tools | PASS | exercised over stdio alongside forget |

## Notes

- CLI has no `forget` subcommand (only `purge`); `forget` is MCP-tool-only. Tested there instead.
- MCP `remember` arg is `content` (not `text`); error path returned isError correctly on wrong arg.
- Build tree contained pre-existing uncommitted edits to crates/remem-recall/src (lib.rs, rank.rs) and tests from a sibling session; built as-is, no source modified by this check.
