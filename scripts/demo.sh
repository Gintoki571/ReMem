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

echo "=== demo OK ==="
