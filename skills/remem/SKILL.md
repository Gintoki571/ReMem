---
name: remem
description: Persistent long-term memory store for agents. Save facts, decisions, mistakes, and preferences after meaningful work; recall them in future sessions by meaning. Use when starting a task ("what do I know about X"), after fixing a bug, making a decision, or discovering a durable fact.
---

# Remem

Local long-term memory (v3 Rust binary, SQLite+FTS5 + local embeddings). DB: `$REMEM_DB` (default `~/.remem/remem.db`); `--db` overrides. Build: `cargo build` in `/home/bindesh/prime-agent/remem` (binary `./target/debug/remem`).

## CLI

```bash
./target/debug/remem remember <kind> "<content>" --tags t1,t2 --agent <name> --session <name> --importance 0.8
./target/debug/remem recall "<query>" --k 5 --json --agent <name>
./target/debug/remem link <fromId> <toId> [--rel REL]
```

Kinds: fact | decision | mistake | preference | event | note. `--tags` comma-separated; dates (`--occurred-at`, `--since`, `--until`) take unix seconds or YYYY-MM-DD. `list` takes no `--k`.

CLI mirrors MCP tools 1:1: list, forget <id> (soft-delete), purge <id> (hard-delete), related <id>, central, path <from> <to>, stats, validate.

## When to save

- Non-obvious bug fix: kind=mistake, importance >= 0.8, tag the component.
- Design choice with rationale: kind=decision.
- Environment fact (paths, versions, quirks): kind=fact.
- User preference: kind=preference.

## When to recall

- Task start, when prior memory may exist.
- Before debugging: recall the component name.
- After a spawn/tool failure: recall the tool/provider name.

## Conventions

- One memory = one atomic fact. Tags: lowercase component name. importance: 0.5 default, 0.9+ for costly lessons. Set --agent/--session so memories are attributable and filterable.

## MCP tools

Persistent agents: use MCP tools, not the CLI. Registration: `docs/prime-agent-integration.md`.

- `remember(kind, content, tags?, agent?, session?, importance?)`
- `recall(query, k?, agent?, session?, maxChars?, minScore?)`
- `list(limit?)` / `link(from, to, rel?)` / `forget(id)` / `purge(id)`
- `stats()` / `validate()` (empty = healthy)
- `related(id, rel?)` / `central(limit?)` / `path(from, to)`

Near-duplicate save returns `similar: [id dist, ...]` (L2 < 0.48): link ids worth keeping, or purge the new id. Recall floor off by default; opt in `minScore: 0.02` only to suppress junk (see `docs/floor-decision.md`).
