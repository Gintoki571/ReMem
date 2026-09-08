# ReMem v3 — prime-agent integration (MCP stdio)

No Rust code. Config and docs only.

## 1. How prime-agent registers MCP servers

Source: `packages/coding-agent/docs/mcp-integrations.md` ("Generic MCP servers").

- Config lives in the **user** settings file only: `~/.prime/agent/settings.json`
  under the `mcpServers` key.
- Managed with the CLI (exits without starting an agent) or the TUI `/mcp` command:
  `prime-agent mcp add/list/get/remove`.
- Project-level `.prime/agent/settings.json` MCP entries are **ignored for execution**,
  so a repo cannot start a local process. Do not document a project-local registration.
- Generic servers are called from the Python kernel via the pre-imported `mcp` module,
  not as agent tools:
  `tools = await mcp.list_tools("remem")`.
- stdio `env` accepts only tagged references to existing environment variables
  (`{"env": "VAR"}`); literal secret values are rejected. Set `REMEM_DB` in the
  ambient environment before launching prime-agent, then reference it.

## 2. Register remem-mcp

Build once (any machine with a working toolchain):

```bash
cargo build --release -p remem-mcp
```

Export the DB path in every shell that launches prime-agent:

```bash
export REMEM_DB="$HOME/.remem/remem.db"
```

Register (run from the remem checkout; uses the absolute binary path):

```bash
prime-agent mcp add local --cwd /home/bindesh/prime-agent/remem \
  --env REMEM_DB=REMEM_DB -- /home/bindesh/prime-agent/remem/target/release/remem-mcp
```

Resulting entry in `~/.prime/agent/settings.json` (written by the CLI, do not hand-edit):

```json
{
  "mcpServers": {
    "remem": {
      "type": "stdio",
      "command": "/home/bindesh/prime-agent/remem/target/release/remem-mcp",
      "args": [],
      "cwd": "/home/bindesh/prime-agent/remem",
      "env": { "REMEM_DB": { "env": "REMEM_DB" } }
    }
  }
}
```

Notes:

- Server binary takes **no CLI args**; DB path comes only from `REMEM_DB`
  (default `~/.remem/remem.db`, `~/` expanded via `$HOME`). See
  `crates/remem-mcp/src/main.rs` (`db_path()`).
- Server speaks newline-delimited JSON-RPC 2.0 over stdio (`initialize`,
  `tools/list`, `tools/call`); it needs no `--stdio` flag, so `args` is empty.
  Use `target/debug/remem-mcp` for dev, `target/release/remem-mcp` for daily use.
- Tools exposed: `remember`, `recall`, `list`, `link`, `stats`, `validate`.
- Name `remem` is not a built-in (`linear`, `notion`, ...) so it is safe to use.

## APPLY-BY-HUMAN (exact diff — do not apply automatically)

No prime-agent repo files are modified by this doc. To register, the human runs
the `prime-agent mcp add local ...` command above, which produces this diff in
`~/.prime/agent/settings.json`:

```diff
 {
   "mcpServers": {
+    "remem": {
+      "type": "stdio",
+      "command": "/home/bindesh/prime-agent/remem/target/release/remem-mcp",
+      "args": [],
+      "cwd": "/home/bindesh/prime-agent/remem",
+      "env": { "REMEM_DB": { "env": "REMEM_DB" } }
+    }
   }
 }
```

Precondition: `export REMEM_DB="$HOME/.remem/remem.db"` in the launching shell.

## 3. Verify

```bash
prime-agent mcp list
prime-agent mcp get remem
```

Expected: `remem` listed; `get` shows the stdio command above.

From inside a prime-agent session (Python kernel):

```python
tools = await mcp.list_tools("remem")
print([t["name"] for t in tools])
# expect: ['remember', 'recall', 'list', 'link', 'stats', 'validate']
result = await mcp.call_tool("remem", "stats", {})
print(result)
```

If the call fails, run `await mcp.reload()` and retry; check that `REMEM_DB`
was exported before prime-agent started.
