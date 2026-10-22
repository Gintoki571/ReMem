---
name: neocortex
description: Long-term memory for AI agents. Use when you should remember a durable fact, decision, preference, mistake or outcome, or when you need to recall what was known earlier (people, projects, environment, past work, past decisions).
---

# neocortex

neocortex is a local long-term memory that survives across sessions. It stores
memories, entities, relations and time-bounded facts in one SQLite file, and
retrieves with keyword, vector, graph and temporal search. You are the only
writer: nothing is inferred beyond what you pass.

## When to remember

Store something when it will still matter next week:

- decisions and **why** they were made (rejected alternatives included)
- user preferences, constraints and working style
- stable facts about people, places, projects, tools and the environment
- mistakes and the fix that worked, so they are not repeated
- procedures that worked (build steps, deploy order, gotchas)
- outcomes: what shipped, what failed, what was measured
- dates of events and when a fact became true

## When NOT to remember

- transient chatter, intermediate reasoning, things already in the repo
- secrets, tokens, passwords, credentials, private keys
- content you can re-read from a file, issue or commit
- near-duplicates: recall first, and update instead of adding another copy

## How to write a good memory

- **Atomic**: one fact per memory. "Deploy moved to Friday" and "Friday deploy
  needs sign-off" are two memories.
- **Self-contained**: no "it", "this", "yesterday". Name the subject and use
  absolute dates ("2026-09-14", not "last week").
- **Consistent names**: always "Mom", never "mother" in one memory and "Mom" in
  the next. Aliases belong in `entities`.
- **Structured when known**: pass `entities` and `relations`. Mark single-valued
  relations (`lives in`, `works at`, `is married to`, `has phone number`)
  `exclusive: true` so the previous value is ended and kept as history.
  Exclusivity sticks to the predicate for every future write, so never mark
  multi-valued ones (`uses`, `knows`, `likes`, `depends on`) exclusive.
- **Correct, don't contradict**: use `supersedes: ["m12"]` for a newer memory
  that replaces an older one, or `invalidate` when a fact stopped being true.
  Never write two live facts that disagree.
- Pick a `kind`: `fact`, `event`, `decision`, `preference`, `mistake`,
  `procedure`, `observation`, `note`. Add `tags` for filtering and
  `importance` (0..1, ~0.7+ for things the user will ask about again).

## When to recall

- at the start of a task, before planning
- before a decision that may depend on past context
- when the user refers to something from before ("as we discussed", "last time")
- when stuck: the answer or a past fix may already be stored

## How to read results

- `[m42]` memory, `[r9]` relation, `[e3]` entity. Pass ids back with or without
  the prefix.
- A date range with `[ended]` or "ended" is **history**: true then, not now.
  Current facts have `since <date>` or no end.
- `(bm25#2 vec#1 graph#1)` says which channels matched; more agreeing channels
  means a stronger hit.
- "no memories matched" means broaden the query or drop filters, not that the
  memory does not exist.

## Tools (MCP) and CLI equivalents

| MCP tool | CLI | Use |
|---|---|---|
| `remember` | `neocortex remember <text>` | store a memory |
| `recall` | `neocortex recall <query>` | retrieve |
| `entity` | `neocortex entity <name\|eID>` | profile of one entity |
| `invalidate` | `neocortex invalidate <mID\|rID> --at <date>` | end a fact |
| `forget` | `neocortex forget <mID> [--hard]` | retire/delete |
| `merge_entities` | `neocortex merge <keep> <merge>` / `alias <entity> <alias>` | fix names |
| `overview` | `neocortex overview [query]` | clusters of the graph |
| `stats` | `neocortex stats` | counts and model status |

Every tool takes `format: "text"` (default) or `"json"`.
Use `overview` for a compact global view of related entities, facts and a
representative memory; use `entity` for one entity's facts and timeline.

## Examples

Store a decision with structure:

```json
{"content": "Chose SQLite over Postgres for neocortex because it ships as one file with no server.",
 "kind": "decision", "tags": ["architecture", "storage"], "importance": 0.8,
 "entities": [{"name": "neocortex", "type": "project"},
              {"name": "SQLite", "type": "tool"}, {"name": "Postgres", "type": "tool"}],
 "relations": [{"source": "neocortex", "predicate": "uses", "target": "SQLite"}],
 "keywords": ["storage choice"]}
```

Record a change instead of a contradiction:

```json
{"content": "Mom moved from Delhi to Pune on 2026-09-14.",
 "kind": "event", "tags": ["family"], "valid_from": "2026-09-14",
 "entities": [{"name": "Mom", "type": "person", "aliases": ["mother"]},
              {"name": "Pune", "type": "place"}],
 "relations": [{"source": "Mom", "predicate": "lives in", "target": "Pune", "exclusive": true}]}
```

Recall before deciding:

```
neocortex recall "how do we deploy" --tags deploy
neocortex recall "what did we decide about storage" --history
```

CLI, same memory:

```bash
neocortex remember "Chose SQLite over Postgres for neocortex: one file, no server." \
  --kind decision --tags architecture,storage --importance 0.8
neocortex recall "storage choice" --k 5 --json
```

Write absolute dates; neocortex resolves relative dates in the machine's local
timezone.
