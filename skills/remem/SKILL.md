---
name: remem
description: Persistent long-term memory store for agents. Save facts, decisions, mistakes, and preferences after meaningful work; recall them in future sessions by meaning. Use when starting a task ("what do I know about X"), after fixing a bug, making a decision, or discovering a durable fact.
---

# Remem

Local long-term memory (v3 Rust binary) backed by SQLite+FTS5, local BERT embeddings, and a graphqlite graph projection in the same DB file.

## CLI

```bash
export REMEM_DB="$HOME/.remem/remem.db"
cd /home/bindesh/prime-agent/remem
cargo build   # binary at ./target/debug/remem

# save a memory (kinds: fact | decision | mistake | preference | event | note)
./target/debug/remem remember <kind> "<content>" --tags t1,t2 --agent <name> --session <name> --importance 0.8 --occurred-at 2026-09-01

# recall by meaning (vector + keyword fusion)
./target/debug/remem recall "<natural language query>" --k 5
./target/debug/remem recall "<query>" --k 5 --json --agent <name> --session <name>
./target/debug/remem recall "<query>" --k 5 --since 2026-08-01 --until 2026-09-08

# other commands
./target/debug/remem list [--json]
./target/debug/remem link <fromId> <toId> [--rel REL]
./target/debug/remem purge <id>  # hard-delete: row, FTS entry, embedding, graph node
./target/debug/remem stats
./target/debug/remem validate  # store/graph consistency, exit 1 if issues
```

Notes: `--db` overrides the DB path (env `REMEM_DB`, default `~/.remem/remem.db`). `--tags` is comma-separated. `--occurred-at`, `--since`, `--until` take unix seconds or YYYY-MM-DD. `list` takes no `--k`. MCP tools: remember, recall, list, link, forget (by id), stats, validate.

`remember` prints a second `similar: [id dist, ...]` line (MCP: `similar` array) when the new text is a near-duplicate of stored memories (L2 < 0.48). It means re-save, not new fact: `link` the ids if both are worth keeping, or `purge` the new id if redundant. Exact duplicates dedup silently (existing id, no `similar` line).

Recall floor is off by default. Opt in with `recall --min-score 0.02` (MCP: `minScore: 0.02`) to suppress near-zero-score junk. 0.02 is the calibrated value; see `docs/floor-decision.md`. The floor can drop weak real hits (Q27/Q28), so leave it off unless junk suppression matters.

## When to save

- After fixing a non-obvious bug: kind=mistake, importance >= 0.8, tags include the component.
- After a design choice with a rationale: kind=decision.
- After discovering an environment fact (paths, versions, provider quirks): kind=fact.
- User preferences: kind=preference.

## When to recall

- At task start when touching an area you may have memory about.
- Before debugging: recall the component name.
- When a spawn/tool fails: recall the tool/provider name.

## Conventions

- One memory = one atomic fact. No essays.
- Tags: component name, lowercase, comma-separated.
- importance: 0.5 default, 0.9+ for "cost me an hour" lessons.
- Set --agent to the agent name (prime, or the subagent name) so memories are attributable.
- Set --session when memories belong to one task thread, and filter recall with --session.

## MCP server

Persistent agents: use MCP tools, not the CLI. Shell scripts and one-off
terminal work: use the CLI above.

Registration: see `docs/prime-agent-integration.md` (stdio entry in
`~/.prime/agent/settings.json`, DB path via `REMEM_DB` env only).

Tools (verified against `crates/remem-mcp/src/main.rs` `tools_list()`):

- `remember(kind, content, tags?, agent?, session?, importance?)` - store a memory.
- `recall(query, k?, agent?, session?, maxChars?, minScore?)` - ranked recall.
- `list(limit?)` - newest memories first.
- `link(from, to, rel?)` - link two memories in the graph.
- `forget(id)` - delete a memory by id.
- `purge(id)` - hard-delete row, FTS and vector entries.
- `stats()` - database and graph counts.
- `validate()` - store/graph consistency issues (empty means healthy).
- `related(id, rel?)` - graph neighbors as [{id, rel}].
- `central(limit?)` - top PageRank memories as [{id, score}].
- `path(from, to)` - shortest memory-to-memory path (empty if unreachable).
