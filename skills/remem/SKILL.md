---
name: remem
description: Persistent long-term memory store for agents. Save facts, decisions, mistakes, and preferences after meaningful work; recall them in future sessions by meaning. Use when starting a task ("what do I know about X"), after fixing a bug, making a decision, or discovering a durable fact.
---

# Remem

Local long-term memory backed by SQLite+FTS5, local ONNX embeddings, and LatticeDB.

## CLI

```bash
export REMEM_DATA_DIR="$HOME/.remem-agent"
cd /home/bindesh/prime-agent/remem

# save a memory (kinds: fact | decision | mistake | preference | event | note)
npx tsx src/cli/cli.ts remember <kind> "<content>" --tags t1,t2 --agent <name> --importance 0.8

# recall by meaning (semantic + keyword fusion)
npx tsx src/cli/cli.ts recall "<natural language query>" --k 5

# other commands
npx tsx src/cli/cli.ts list --k 20
npx tsx src/cli/cli.ts link <fromId> <toId> --type CAUSED_BY
npx tsx src/cli/cli.ts events --after 0
npx tsx src/cli/cli.ts stats
```

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
