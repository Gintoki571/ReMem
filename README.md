# neocortex

Long-term memory for AI agents. neocortex stores what an agent learned —
facts, decisions and their reasons, preferences, mistakes and their fixes,
procedures, events — in one SQLite file, and gives it back compactly when it
matters. It is a memory, not a code-retrieval tool: people, places, projects
and how they changed over time.

- One binary: a CLI and an MCP stdio server.
- One SQLite file: BM25 (FTS5), vectors (sqlite-vec), an entity/relation graph
  (graphqlite) and bi-temporal facts.
- Pure Rust embeddings (UltraBERT-Embed-150M on candle, CPU). No Python, no
  server, no network, no LLM inside — the calling agent is the LLM.

## Build

```bash
cargo build --release
# target/release/neocortex
```

## Model setup

Embeddings need the `UltraBERT-Embed-150M` model directory (safetensors fp32,
`config.json`, `tokenizer.json`). Put it at the default location:

```bash
mkdir -p ~/.neocortex/models
cp -r /path/to/UltraBERT-Embed-150M ~/.neocortex/models/
```

or point at it explicitly:

```bash
export NEOCORTEX_MODEL_DIR=/path/to/UltraBERT-Embed-150M
```

The model is loaded lazily: the MCP server starts immediately and loads it on
the first `remember`/`recall`. For tests and wiring checks you can skip the
model entirely:

```bash
export NEOCORTEX_EMBEDDER=hash   # deterministic test embedder, not for real use
```

## Quick start

```bash
neocortex remember "Mom moved from Delhi to Pune on 2026-09-14." \
  --kind event --tags family \
  --entities '[{"name":"Mom","type":"person","aliases":["mother"]},
               {"name":"Pune","type":"place"}]' \
  --relations '[{"source":"Mom","predicate":"lives in","target":"Pune","exclusive":true}]'

neocortex recall "where does Mom live"
```

```
memories (1 of 1, ~90/1200 tokens):
[m1] event 2026-09-14 · Mom moved from Delhi to Pune on 2026-09-14.  (bm25#1 vec#1 graph#1)
facts:
[r1] Mom —live in→ Pune (since 2026-09-14)
entities: [e1] Mom (person; aka mother) · [e2] Pune (place)
```

`--json` on any command returns the full output struct.

## CLI

| command | what it does |
|---|---|
| `remember <content...>` | store a memory; flags below |
| `recall <query...>` | retrieve memories, facts and entities |
| `entity <name\|eID>` | profile, current/past facts and timeline of one entity |
| `invalidate <mID\|rID>` | end a memory or relation at a time, keeping it as history |
| `forget <mID>` | retire a memory (`--hard` deletes it with vectors and mentions) |
| `merge <keep> <merge>` | merge one entity into another |
| `alias <entity> <alias>` | add another name to an entity |
| `overview [query...]` | clusters of the entity graph |
| `stats` | counts, database size, embedder |
| `reindex` | rebuild the graph projection from the relational tables |
| `validate` | report integrity problems (exit 1 when any) |
| `cypher <query>` | read-only Cypher over the graph projection |
| `import-remem <path>` | one-shot import of a ReMem v3 database |
| `export [--out FILE]` | write memories as JSON Lines (`--include-forgotten`) |
| `import <file.jsonl>` | replay a JSON Lines export through `remember` |
| `bench` | synthetic remember/recall latency report |
| `mcp` | run the MCP stdio server |

`remember` flags: `--kind --tags --importance --scope --source --session --when
--until --entities JSON --relations JSON --keywords --supersedes`.
`remember -` reads a whole `RememberInput` JSON object from stdin.
`recall` flags: `--k --budget --kinds --tags --scope --source --session --since
--until --as-of --history --entities --keywords`.

Exit codes: `0` ok, `1` runtime error, `2` usage error.

### Export, import and benchmarks

`neocortex export [--out FILE] [--include-forgotten]` writes one JSON object per
memory, in id order, to `FILE` or stdout. Each line is a `RememberInput` plus
`id` and `expired`, so a single line can be replayed with `neocortex remember -`.
A line carries the memory's entities (name, type, description, aliases), the
relations it is evidence for (canonical predicate, fact, validity, exclusive
flag), validity times as ISO-8601 UTC, and `supersedes` (the ids it replaced).
Superseded rows are exported too, marked `"expired": true`, so the history
survives; forgotten rows are left out unless `--include-forgotten` is given.

`neocortex import FILE.jsonl` replays a file through `remember` in file order
and translates `m<id>` references in `supersedes` to the ids the new database
assigns, then retires the rows that were forgotten. Use it to restore a backup
or to move a database to another embedding model: export with the old model,
import with the new one (a database records the embedder that built it).

`neocortex bench [--n 10000] [--queries 200] [--db PATH]` generates a
deterministic synthetic corpus (xorshift seed, ~300 fictional people, places,
projects and tools, 20% with structured entities and relations, dates spread
over three years) and reports remember and recall p50/p95, remember throughput,
database size and peak RSS. It needs an empty database: without `--db` it uses
a temporary file and deletes it afterwards. `NEOCORTEX_EMBEDDER=hash` runs the
whole benchmark without the model.

## MCP registration

Claude Code:

```bash
claude mcp add neocortex -s user \
  -e NEOCORTEX_DB=$HOME/.neocortex/neocortex.db \
  -- /path/to/neocortex mcp
```

GitHub Copilot CLI (`~/.copilot/mcp-config.json`):

```json
{
  "mcpServers": {
    "neocortex": {
      "command": "/path/to/neocortex",
      "args": ["mcp"],
      "env": { "NEOCORTEX_DB": "/home/you/.neocortex/neocortex.db" }
    }
  }
}
```

opencode (`opencode.json`):

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "neocortex": {
      "type": "local",
      "command": ["/path/to/neocortex", "mcp"],
      "enabled": true,
      "environment": { "NEOCORTEX_DB": "/home/you/.neocortex/neocortex.db" }
    }
  }
}
```

Any other MCP client (`mcp.json`):

```json
{
  "mcpServers": {
    "neocortex": {
      "command": "/path/to/neocortex",
      "args": ["mcp"],
      "env": { "NEOCORTEX_DB": "/home/you/.neocortex/neocortex.db" }
    }
  }
}
```

The server speaks JSON-RPC 2.0 over stdio, one JSON message per line, and
supports protocol versions `2025-06-18`, `2025-03-26` and `2024-11-05`.

## Tools

`remember`, `recall`, `entity`, `invalidate`, `forget`, `merge_entities`,
`overview`, `stats`. Every tool accepts `format: "text"` (default, compact) or
`"json"`. Tool errors come back as `isError: true` content with a message that
says what to fix.

The agent-facing guide — when to remember, how to write a good memory, when to
recall — is in [`skills/neocortex/SKILL.md`](skills/neocortex/SKILL.md).

## How retrieval works

Four channels run in parallel: **BM25** over memory text and tags, **vector**
KNN over UltraBERT embeddings, **graph** personalized PageRank over entities,
memories and relations, and **temporal** overlap with a time window parsed from
the query. Their ranked lists are fused with weighted RRF, then lightly
adjusted for recency, importance and whether a fact is ended. MMR removes
near-duplicates and the result is packed into a token budget, whole items only.

Facts are bi-temporal: `valid_from`/`valid_to` say when something was true in
the world, `expired_at` says when neocortex retired it. Changed facts are kept
as history and are never presented as current unless you ask for history or an
`as_of` snapshot.

## Environment

| variable | default | meaning |
|---|---|---|
| `NEOCORTEX_DB` | `~/.neocortex/neocortex.db` | database file (`--db` overrides) |
| `NEOCORTEX_MODEL_DIR` | `~/.neocortex/models/UltraBERT-Embed-150M` | embedding model directory |
| `NEOCORTEX_TZ` | `UTC` | local offset for relative dates, `+HH:MM` or `Z` |
| `NEOCORTEX_MAX_TOKENS` | `1024` | max tokens per embedding |
| `NEOCORTEX_EMBEDDER` | unset | `hash` selects the deterministic test embedder |

## License

MIT
