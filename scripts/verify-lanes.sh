#!/usr/bin/env bash
# verify-lanes.sh — per-lane verification harness for ReMem v3.1 replay.
# Gates encoded from /home/bindesh/rag/reports/verify-v31-{a,b,c,d,e,f}.md
# (verifier reports, authoritative over builder v31-*.md claims).
#
# Usage: scripts/verify-lanes.sh [lane ...] [--eval] [--eval-only LANE]
#   No args = all lanes except the eval gate (eval needs a debug build +
#   ~5 min embedder run; pass --eval to include it).
#   Each lane assumes the current checkout contains that lane's changes.
#
# Policy: assertions are MINIMUM passed + ZERO failed (exact totals from the
# reports are quoted in comments; sibling lanes legitimately add tests, so
# exact-total equality would false-fail on composite trees).
# Known-accepted REDs warn, never fail: lane A fmt (2 long test lines),
# lane D engine spike-winner (lane F k=120 residue), lane F tree fmt nit.
set -uo pipefail
cd "$(dirname "$0")/.."

PASS=0; FAIL=0; WARN=0
RUN_EVAL=0
LANES=()
for a in "$@"; do
  case "$a" in
    --eval) RUN_EVAL=1 ;;
    a|b|c|d|e|f) LANES+=("$a") ;;
    *) echo "unknown arg: $a (want a|b|c|d|e|f|--eval)"; exit 2 ;;
  esac
done
[ "${#LANES[@]}" -eq 0 ] && LANES=(a b c d e f)

ok()   { PASS=$((PASS+1)); echo "PASS: $1"; }
bad()  { FAIL=$((FAIL+1)); echo "FAIL: $1"; }
warn() { WARN=$((WARN+1)); echo "WARN(accepted): $1"; }

# suite <desc> <min-passed> <cmd...>: exit 0 + >=min passed + 0 failed.
suite() {
  local desc=$1 min=$2; shift 2
  local out rc total failed
  out=$("$@" 2>&1); rc=$?
  total=$(grep -oE '[0-9]+ passed' <<<"$out" | grep -oE '[0-9]+' | awk '{s+=$1} END {print s+0}')
  failed=$(grep -oE '[0-9]+ failed' <<<"$out" | grep -oE '[0-9]+' | awk '{s+=$1} END {print s+0}')
  if [ "$rc" -ne 0 ] || [ "$failed" -ne 0 ] || [ "$total" -lt "$min" ]; then
    bad "$desc (rc=$rc passed=$total want>=$min failed=$failed)"
    tail -n 15 <<<"$out"
  else
    ok "$desc (passed=$total failed=0)"
  fi
}

fmt_check() { # fmt_check <lane>: lane A has a known-accepted fmt RED.
  local lane=$1 out rc
  out=$(cargo fmt --all -- --check 2>&1); rc=$?
  if [ "$rc" -eq 0 ]; then ok "fmt clean"; return; fi
  if [ "$lane" = a ] && grep -q 'crates/remem-recall/tests/cli.rs' <<<"$out"; then
    warn "fmt RED is the accepted lane-A test-only wrap (cli.rs:882,889)"
  else
    bad "fmt dirty"; head -n 20 <<<"$out"
  fi
}

# Eval gate (global, all lanes): 35/37 @1, 37/37 @5, 3/3 adversarial.
eval_gate() {
  local db=${1:-/tmp/remem-verify-eval.db} out
  cargo build -p remem-recall 2>&1 | tail -n 2
  out=$(./scripts/eval.sh "$db" 2>&1); echo "$out" | tail -n 8
  local r1 r5 adv
  r1=$(grep -oE 'recall@1: [0-9]+/37' <<<"$out" | grep -oE '[0-9]+/' | tr -d /)
  r5=$(grep -oE 'recall@5: [0-9]+/37' <<<"$out" | grep -oE '[0-9]+/' | tr -d /)
  adv=$(grep -oE 'adversarial: [0-9]+/3' <<<"$out" | grep -oE '[0-9]+/' | tr -d /)
  [ "${r1:-x}" = 35 ] && ok "eval recall@1 35/37" || bad "eval recall@1 got ${r1:-?}/37 want 35/37"
  [ "${r5:-x}" = 37 ] && ok "eval recall@5 37/37" || bad "eval recall@5 got ${r5:-?}/37 want 37/37"
  [ "${adv:-x}" = 3 ] && ok "eval adversarial 3/3" || bad "eval adversarial got ${adv:-?}/3 want 3/3"
}

lane_a() { # issue #7 correction chain; verifier saw store 51 (6+2+5+5+24+9, incl 9 supersede).
  echo "--- lane A (correction chain) ---"
  suite "store 51+ (supersede 9)" 51 cargo test -p remem-store
  grep -rq 'superseded_at IS NULL' crates/remem-store/schema.sql \
    && ok "partial-index predicate present" || bad "partial-index predicate missing"
  grep -rq 'ORDER BY rowid DESC' crates/remem-store/src/lib.rs \
    && ok "forget_events rowid order" || bad "forget_events rowid order missing"
  fmt_check a
}

lane_b() { # issue #8 CLI wiring; verifier saw recall lib 30 + path 9 + cli 16 + engine 29, all green.
  echo "--- lane B (CLI wiring) ---"
  suite "recall lib 30+" 30 cargo test -p remem-recall --lib
  suite "recall cli 16+" 16 cargo test -p remem-recall --test cli
  suite "recall engine 29+" 29 cargo test -p remem-recall --test engine
  ./target/debug/remem remember --help 2>&1 | grep -q -- '--importance' \
    && bad "remember still accepts --importance" || ok "remember --importance removed"
  fmt_check b
}

lane_c() { # issue #9 floors/hubs/recency; verifier saw recall 107 (37+9+16+3+32+10 lane_c).
  echo "--- lane C (floors/hubs/recency) ---"
  suite "recall workspace 107+" 107 cargo test -p remem-recall
  grep -q 'DEFAULT_COSINE_FLOOR: f32 = 0.63' crates/remem-recall/src/rank.rs \
    && ok "cosine floor 0.63" || bad "DEFAULT_COSINE_FLOOR != 0.63"
  [ ! -e crates/remem-recall/examples/lane_c_calib.rs ] \
    && ok "calib probe deleted" || bad "lane_c_calib.rs still present"
  fmt_check c
}

lane_d() { # issue #10 graph integrity; verifier saw graph 23 + coexistence 3, mcp 6+17.
  echo "--- lane D (graph integrity) ---"
  suite "graph 23+" 23 cargo test -p remem-graph --test graph
  suite "graph coexistence 3" 3 cargo test -p remem-graph --test coexistence
  local out rc failed
  out=$(cargo test -p remem-recall --test engine 2>&1); rc=$?
  failed=$(grep -oE '[0-9]+ failed' <<<"$out" | grep -oE '[0-9]+' | awk '{s+=$1} END {print s+0}')
  if [ "$failed" = 1 ] && grep -q 'default_weights_are_the_spike_winner\|default_rrf_k_is_the_spike_winner' <<<"$out"; then
    warn "engine spike-winner RED is the accepted lane-F residue"
  else
    local total; total=$(grep -oE '[0-9]+ passed' <<<"$out" | grep -oE '[0-9]+' | awk '{s+=$1} END {print s+0}')
    [ "$rc" -eq 0 ] && [ "$failed" = 0 ] && [ "$total" -ge 31 ] \
      && ok "recall engine 31+ clean" \
      || { bad "recall engine (rc=$rc passed=$total failed=$failed)"; tail -n 10 <<<"$out"; }
  fi
  suite "mcp 23+ (6 unit + 17 stdio)" 23 cargo test -p remem-mcp
  fmt_check d
}

lane_e() { # issue #11 live_filter + corroboration; verifier saw 4/4 + 3/3 + lib 37.
  echo "--- lane E (live_filter + corroboration) ---"
  suite "live_filter 4/4" 4 cargo test -p remem-store --test live_filter_test
  suite "corroboration 3/3" 3 cargo test -p remem-recall --test corroboration
  suite "recall lib 37+" 37 cargo test -p remem-recall --lib
  grep -rn 'const OFF' crates/ | grep -qv 'OFFLINE\|COFF' \
    && bad "kill-switch const OFF still present" || ok "kill-switch gone"
  fmt_check e
}

lane_f() { # issue #12 spikes+docs; verifier saw sentinel 5/5 + types 1, k reverted to 30.
  echo "--- lane F (spikes + docs) ---"
  suite "sentinel 5/5" 5 cargo test -p remem-store --test sentinel_test
  suite "types 1+" 1 cargo test -p remem-types
  grep -q 'DEFAULT_RRF_K.*= 30' crates/remem-recall/src/rank.rs \
    && ok "RRF k reverted to 30" || bad "DEFAULT_RRF_K != 30 (spike residue?)"
  [ -e docs/doubt-scratchpad.md ] && ok "doubt-scratchpad present" || bad "doubt-scratchpad missing"
  local out rc
  out=$(cargo fmt --all -- --check 2>&1); rc=$?
  if [ "$rc" -eq 0 ]; then ok "fmt clean";
  elif [ "$(grep -c '^Diff' <<<"$out")" = "$(grep -c 'lane_c_calib' <<<"$out")" ]; then
    warn "tree fmt nit is lane-C calib file only, not lane F"
  else bad "fmt dirty beyond lane-C nit"; head -n 10 <<<"$out"; fi
}

for l in "${LANES[@]}"; do "lane_$l"; done
if [ "$RUN_EVAL" -eq 1 ]; then echo "--- eval gate (35/37 @1, 37/37 @5, 3/3 adv) ---"; eval_gate; fi

echo; echo "lanes(${LANES[*]}) PASS=$PASS FAIL=$FAIL WARN=$WARN eval=$([ "$RUN_EVAL" -eq 1 ] && echo on || echo off)"
[ "$FAIL" -eq 0 ]
