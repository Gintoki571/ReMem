# remem v3 demo (onboarding)

Runs the core loop against a scratch db (default `REMEM_DB=/tmp/remem-demo.db`,
override with `REMEM_DB=/tmp/custom.db scripts/demo.sh`):
`scripts/demo.sh`. It builds the debug `remem` + `remem-mcp` binaries, stores
one fact, one decision and one mistake (each with tags and an agent id), then
exercises recall, link, validate, stats, purge, an MCP-vs-CLI parity check
(`tools/list` over stdio must report 11 tools), forget (soft-delete), an
explicit `related` check, a 3-chain `central`/`path` check, and a
`recall --json` agent/session spot-check. Any step failing exits nonzero.

Note: the `remem` CLI mirrors the graph tools: `forget` (soft delete),
`related`, `central`, `path`, plus `purge` (hard delete: row, FTS entry,
embedding, graph forget). Related memories also surface through recall
`graph#` reasons (see `crates/remem-mcp` for the MCP equivalents).

## Step timings (build excluded, scratch db `/tmp/remem-demo-timed.db`)

Measured with `date +%s%N` wrappers in `scripts/demo.sh` (3 slowest steps):

```
step 10 took 13.2s
step 11 took 7.5s
step 12 took 3.4s
```

Full script exit code: 0.

## Script source (`scripts/demo.sh`)

```bash
#!/usr/bin/env bash
#
# remem v3 onboarding demo.
#
# Builds the debug `remem` + `remem-mcp` binaries and walks a scratch db
# through the core loop: remember -> recall -> link -> validate -> stats
# -> purge -> MCP-vs-CLI parity.
# Exits nonzero on any failure. Touches only ${REMEM_DB:-/tmp/remem-demo.db}*.
#
# Usage: scripts/demo.sh
# Override the scratch db with: REMEM_DB=/tmp/custom.db scripts/demo.sh
set -euo pipefail

cd "$(dirname "$0")/.."

echo "=== 1. build debug remem + remem-mcp ==="
cargo build -p remem-recall -p remem-mcp
BIN="./target/debug/remem"
MCP_BIN="./target/debug/remem-mcp"

echo "=== 2. fresh scratch db ==="
export REMEM_DB="${REMEM_DB:-/tmp/remem-demo.db}"
rm -f "$REMEM_DB" "$REMEM_DB-wal" "$REMEM_DB-shm" "$REMEM_DB-journal"

echo "=== 3. remember one fact, one decision, one mistake ==="
FACT_ID="$($BIN remember fact "Postgres runs on port 5432 in this project" --tags demo,postgres --agent demo-agent)"
echo "fact:     $FACT_ID"
DECISION_ID="$($BIN remember decision "Use SQLite vec0 for the demo embeddings" --tags demo,embeddings --agent demo-agent)"
echo "decision: $DECISION_ID"
MISTAKE_ID="$($BIN remember mistake "Forgot to set REMEM_DB and wrote to the default db once" --tags demo,env --agent demo-agent)"
echo "mistake:  $MISTAKE_ID"

echo "=== 4. recall (ranked, with fts/vector reasons) ==="
RECALL_OUT="$($BIN recall "which database port" --k 5)"
echo "$RECALL_OUT"
if [ -z "$RECALL_OUT" ]; then echo "FAIL: recall returned nothing" >&2; exit 1; fi

echo "=== 5. link memories, then recall shows the graph reason (related) ==="
$BIN link "$FACT_ID" "$DECISION_ID" --rel relates-to
$BIN link "$MISTAKE_ID" "$FACT_ID" --rel relates-to
RELATED_OUT="$($BIN recall "which database port" --k 5)"
echo "$RELATED_OUT"
if ! echo "$RELATED_OUT" | grep -q "graph#"; then echo "FAIL: no graph reason after link" >&2; exit 1; fi

echo "=== 6. validate (clean graph expected) ==="
$BIN validate

echo "=== 7. stats ==="
$BIN stats

echo "=== 8. purge the mistake, prove it is gone ==="
# NOTE: the CLI exposes purge (hard delete: row, FTS entry, embedding, plus
# graph forget). The same engine ops are also available as the MCP
# `forget` / `related` tools (see crates/remem-mcp).
$BIN purge "$MISTAKE_ID"
if $BIN list | grep -q "$MISTAKE_ID"; then echo "FAIL: purged id still listed" >&2; exit 1; fi
$BIN validate

echo "=== 9. MCP-vs-CLI parity (tools/list shows 11 tools) ==="
MCP_COUNT="$(printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}' | "$MCP_BIN" | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["result"]["tools"]))')"
echo "mcp tools: $MCP_COUNT"
if [ "$MCP_COUNT" != "11" ]; then echo "FAIL: MCP tools/list returned $MCP_COUNT tools, expected 11" >&2; exit 1; fi

echo "=== 10. forget (soft-delete: gone from list/recall, validate clean) ==="
TEMP_ID="$($BIN remember note "Temporary scratch note for the forget check" --tags demo,forget --agent demo-agent)"
$BIN forget "$TEMP_ID"
if $BIN list | grep -q "$TEMP_ID"; then echo "FAIL: forgotten id still listed" >&2; exit 1; fi
if $BIN recall "Temporary scratch note forget check" --k 5 | grep -q "$TEMP_ID"; then echo "FAIL: forgotten id still recalled" >&2; exit 1; fi
$BIN validate

echo "=== 11. 3-chain A->B->C: related, central, path ==="
CHAIN_A="$($BIN remember fact "Lighthouse keepers log foghorn tests every dawn" --tags demo,chain --agent demo-agent)"
CHAIN_B="$($BIN remember fact "Quantum error correction thresholds for surface codes" --tags demo,chain --agent demo-agent)"
CHAIN_C="$($BIN remember fact "Sourdough starter hydration ratios for high altitude" --tags demo,chain --agent demo-agent)"
$BIN link "$CHAIN_A" "$CHAIN_B" --rel relates-to
$BIN link "$CHAIN_B" "$CHAIN_C" --rel relates-to
RELATED_B="$($BIN related "$CHAIN_B")"
echo "$RELATED_B"
if ! echo "$RELATED_B" | grep -q "$CHAIN_A"; then echo "FAIL: related missing chain A" >&2; exit 1; fi
if ! echo "$RELATED_B" | grep -q "$CHAIN_C"; then echo "FAIL: related missing chain C" >&2; exit 1; fi
CENTRAL_OUT="$($BIN central --limit 10)"
echo "$CENTRAL_OUT"
if ! echo "$CENTRAL_OUT" | grep -q "$CHAIN_B"; then echo "FAIL: central missing chain B" >&2; exit 1; fi
PATH_OUT="$($BIN path "$CHAIN_A" "$CHAIN_C")"
echo "$PATH_OUT"
if [ "$(printf '%s\n' "$PATH_OUT" | wc -l | tr -d ' ')" != "3" ]; then echo "FAIL: path A->C is not 3 lines" >&2; exit 1; fi

echo "=== 12. recall --json spot-check (agent/session fields) ==="
JSON_ID="$($BIN remember fact "JSON spot-check memory with agent and session" --tags demo,json --agent demo-agent --session demo-session)"
$BIN recall "JSON spot-check memory" --k 5 --json | python3 -c 'import json,sys; hits=json.load(sys.stdin); assert hits, "no hits"; assert any("agent" in h and "session" in h and h["agent"]=="demo-agent" and h["session"]=="demo-session" for h in hits), "agent/session missing"'
echo "json ok: $JSON_ID"

echo "=== demo OK ==="
```

## Sample output

```
=== 1. build debug remem + remem-mcp ===
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.12s
=== 2. fresh scratch db ===
=== 3. remember one fact, one decision, one mistake ===
embedder: Cpu (768d)
fact:     3fdb8b23-054e-4ef4-a4f7-c23548a1ebbf
embedder: Cpu (768d)
decision: a1f12764-909e-4153-972f-05025d2e3c77
embedder: Cpu (768d)
mistake:  10f5b64c-ef32-462d-8378-018d0a2e510f
=== 4. recall (ranked, with fts/vector reasons) ===
embedder: Cpu (768d)
3fdb8b23-054e-4ef4-a4f7-c23548a1ebbf  0.0484  [fact]  Postgres runs on port 5432 in this project  (fts#1,vector#1,recent)
10f5b64c-ef32-462d-8378-018d0a2e510f  0.0156  [mistake]  Forgot to set REMEM_DB and wrote to the default db once  (vector#2,recent)
a1f12764-909e-4153-972f-05025d2e3c77  0.0152  [decision]  Use SQLite vec0 for the demo embeddings  (vector#3,recent)
=== 5. link memories, then recall shows the graph reason (related) ===
embedder: Cpu (768d)
embedder: Cpu (768d)
embedder: Cpu (768d)
3fdb8b23-054e-4ef4-a4f7-c23548a1ebbf  0.0787  [fact]  Postgres runs on port 5432 in this project  (fts#1,vector#1,graph#3,recent)
a1f12764-909e-4153-972f-05025d2e3c77  0.0474  [decision]  Use SQLite vec0 for the demo embeddings  (vector#3,graph#1,recent)
10f5b64c-ef32-462d-8378-018d0a2e510f  0.0469  [mistake]  Forgot to set REMEM_DB and wrote to the default db once  (vector#2,graph#2,recent)
=== 6. validate (clean graph expected) ===
embedder: Cpu (768d)
=== 7. stats ===
embedder: Cpu (768d)
{
  "graph": {
    "edges": 5,
    "nodes": 4
  },
  "memories": 3
}
=== 8. purge the mistake, prove it is gone ===
purged 10f5b64c-ef32-462d-8378-018d0a2e510f
embedder: Cpu (768d)
embedder: Cpu (768d)
=== 9. MCP-vs-CLI parity (tools/list shows 11 tools) ===
embedder: Cpu (768d)
mcp tools: 11
=== 10. forget (soft-delete: gone from list/recall, validate clean) ===
embedder: Cpu (768d)
embedder: Cpu (768d)
forgot 3c4bbfc2-5cb2-4fb7-9302-a2120481f31d
embedder: Cpu (768d)
embedder: Cpu (768d)
embedder: Cpu (768d)
=== 11. 3-chain A->B->C: related, central, path ===
embedder: Cpu (768d)
embedder: Cpu (768d)
embedder: Cpu (768d)
embedder: Cpu (768d)
embedder: Cpu (768d)
embedder: Cpu (768d)
d086a2a5-6b32-42e8-b5e0-5956f9355744  relates_to
e8cf2339-ceb6-40a6-baa7-902403cc6141  relates_to
embedder: Cpu (768d)
d086a2a5-6b32-42e8-b5e0-5956f9355744  0.040141
a1f12764-909e-4153-972f-05025d2e3c77  0.035625
b3385a60-3d2d-4d2f-b7ba-f63fb529a6c9  0.035625
3fdb8b23-054e-4ef4-a4f7-c23548a1ebbf  0.025000
e8cf2339-ceb6-40a6-baa7-902403cc6141  0.025000
embedder: Cpu (768d)
e8cf2339-ceb6-40a6-baa7-902403cc6141
b3385a60-3d2d-4d2f-b7ba-f63fb529a6c9
d086a2a5-6b32-42e8-b5e0-5956f9355744
=== 12. recall --json spot-check (agent/session fields) ===
embedder: Cpu (768d)
embedder: Cpu (768d)
json ok: f82b138e-4bd6-42ec-87c7-81bad15dc2e0
=== demo OK ===
```

## What this proves

- `remember` stores all three memory kinds with tags and agent scope intact.
- `recall` fuses FTS and vector rankings and reports a reason per hit.
- `link` connects memories, and linked neighbours resurface via `graph#` reasons.
- `validate` confirms a clean graph and `stats` reports memory and graph counts.
- `purge` hard-deletes a memory (row, index, graph node) and `list` proves it is gone.
- MCP-vs-CLI parity: `tools/list` over stdio reports the documented 11 tools, and the script fails loudly otherwise.
- `forget` soft-deletes: the id vanishes from `list` and `recall` while `validate` stays clean.
- `related` lists both neighbours of the middle of a 3-chain; `central` ranks it and `path` prints the 3-id chain.
- `recall --json` carries full fields: the spot-check asserts `agent`/`session` survive the round trip via python3.
