# Release Check 2

Date: 2026-09-09 (rerun of this check; release binaries rebuilt from a clean release target)
Branch: v3
Git rev: c173f5038aa55d55cbe4be98dddbcf5a0e183e7b (pull: already up to date; HEAD commit c173f50 "test(v3): handshake covers notifications/initialized")
Source changes: none; working tree clean at start and end of this check

## Build

- `cargo clean --release && cargo build --release` (both binaries: `remem` from remem-recall, `remem-mcp`): PASS, clean, no warnings
- Build time: 1m 46s (106.8s wall, measured); an incremental no-op rebuild after that finished in 3.7s
- Binary sizes:
  - `target/release/remem` (remem-recall CLI): 13,860,592 bytes (13.2 MiB)
  - `target/release/remem-mcp`: 13,241,808 bytes (12.6 MiB)

## Demo script on RELEASE binaries

- Ran `scripts/demo.sh` unchanged except `target/debug` -> `target/release` and skipping its own debug `cargo build` step
- Fresh scratch db: `REMEM_DB=/tmp/remem-rel4.db` (db removed before the run)
- Result: PASS, **exit 0**, wall 5.5s. All 12 steps OK, including step 9 (`mcp tools: 11`), step 10 (1.0s), step 11 (1.6s), step 12 (0.5s)
- Embedder reported `Cpu (768d)` throughout

## MCP handshake on the release binary

Driven from Python with `selectors.select` + timeout for every read (never a bare readline after a notification), on a fresh db `/tmp/remem-rel5.db`:

1. `initialize` (id 1) -> reply in 0.15s; `protocolVersion` 2024-11-05, `serverInfo` {remem-mcp, 0.1.0}
2. `notifications/initialized` -> sent fire-and-forget, no reply read (notifications get none)
3. `tools/list` (id 2) -> **11 tools**: remember, recall, list, link, forget, purge, stats, validate, related, central, path
4. `tools/call` stats (id 3) -> returned without error on the same session

Process exited 0.

## Notes

- CLI arg order is `remember <KIND> <TEXT...>` and `recall <QUERY...> --json` (no --text/--query flags).
- MCP `link` args are `from`/`to`; the wrong-arg error path returns `isError` correctly.
- Prior run of this check (rev 608eb79, build 00:47) reported 9.3s build and sizes 13,470,808 / 12,958,752 bytes; that build was incremental on a dirty tree with a sibling session's uncommitted edits. This run is a clean-tree, clean-target measurement, so it is the number to trust for cold-build cost.
