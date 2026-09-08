# Release Check 2

Date: 2026-09-09 (build timestamp 00:47 local)
Branch: v3
Git rev: 608eb79414e9fc593f44da25e0d903c286ff9166 (pull: already up to date; HEAD commit 608eb79 "docs(v3): parity refresh, 4 CLI gaps left")
Source changes: none (uncommitted local edits by sibling session left untouched; built as-is - see note)

## Build

- `cargo build --release -p remem-recall -p remem-mcp`: PASS (9.3s, clean)
- Binary sizes:
  - `target/release/remem` (remem-recall CLI): 13,470,808 bytes (12.8 MiB)
  - `target/release/remem-mcp`: 12,958,752 bytes (12.4 MiB)

## Smoke results (scratch db /tmp/remem-rel3.db via RELEASE binaries)

| Command | Result | Notes |
|---|---|---|
| remember (CLI) | PASS | returned UUID + "embedder: Cpu (768d)"; x2 memories |
| recall (CLI) | PASS | --json ranked hits with fts/vector/recent reasons |
| link (CLI) | PASS | edge created; stats showed 2 nodes / 1 edge |
| validate (CLI) | PASS | exit 0 pre-purge; exit 1 with orphan issue after purge (expected) |
| stats (CLI) | PASS | correct JSON counts before/after purge |
| purge (CLI) | PASS | "purged <id>"; stats dropped to 1 memory / 0 edges |
| forget (MCP tool) | PASS | {"forgotten":true}; stats confirm removal |
| MCP tools/list | PASS | 11 tools over stdio: remember, recall, list, link, forget, purge, stats, validate, related, central, path |
| MCP remember/recall/link/list/stats/validate tools | PASS | recall roundtrip ranked hit; link ok + validate clean; forget removal confirmed |

## Notes

- CLI has a `forget` subcommand now (soft delete; tracker #3 closed); `forget` was MCP-tool-only at check time, so it was tested there instead.
- CLI arg order is `remember <KIND> <TEXT...>` and `recall <QUERY...> --json` (no --text/--query flags).
- MCP `link` args are `from`/`to`; wrong-arg error path returned isError correctly.
- Build tree contained pre-existing uncommitted edits to crates/remem-recall, crates/remem-store, crates/remem-types from a sibling session; built as-is, no source modified by this check.
