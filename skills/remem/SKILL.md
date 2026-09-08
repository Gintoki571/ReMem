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
./target/debug/remem remember <kind> "<content>" --tags t1,t2 --agent <name> --session <name> --importance 0.8

# recall by meaning (vector + keyword fusion)
./target/debug/remem recall "<natural language query>" --k 5
./target/debug/remem recall "<query>" --k 5 --json --agent <name> --session <name>

# other commands
./target/debug/remem list [--json]
./target/debug/remem link <fromId> <toId> [--rel REL]
./target/debug/remem stats
```

Notes: `--db` overrides the DB path (env `REMEM_DB`, default `~/.remem/remem.db`). `--tags` is comma-separated. There is no `validate` subcommand; `list` takes no `--k`.

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
