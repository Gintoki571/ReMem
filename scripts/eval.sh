#!/usr/bin/env bash
# Recall-quality eval: loads docs/eval-fixtures.json via `remem remember`,
# runs each query via `remem recall --k 5 --min-score 0.017`, scores recall@1 / recall@5.
# Answerable queries (non-empty expect) count toward recall@1/@5; adversarial
# queries (empty expect) PASS only when recall returns [] and are reported
# separately, excluded from recall denominators.
# Usage: scripts/eval.sh [db-file]   (default: /tmp/remem-eval.db)
set -euo pipefail
cd "$(dirname "$0")/.."
BIN=./target/debug/remem
DB=${1:-/tmp/remem-eval.db}
FIXTURES=docs/eval-fixtures.json

rm -f "$DB"
export REMEM_DB="$DB"

# Load memories. Prints "idx<TAB>id".
load_memories() {
python3 - "$FIXTURES" <<'PY' | while IFS=$'\t' read -r idx id; do
import json, subprocess, sys
fx = json.load(open(sys.argv[1]))
for i, m in enumerate(fx["memories"]):
    args = ["remember", m["kind"], m["content"],
            "--tags", ",".join(m["tags"]), "--agent", m["agent"],
            "--importance", str(m["importance"])]
    r = subprocess.run(["./target/debug/remem"] + args,
                       capture_output=True, text=True)
    id = r.stdout.splitlines()[0].strip() if r.stdout else ""
    print(f"{i}\t{id}")
PY
echo "$idx $id"
done
}

echo "Loading memories..."
load_memories > /tmp/remem-eval-ids-$$.txt
n_loaded=$(wc -l < /tmp/remem-eval-ids-$$.txt)
echo "Loaded $n_loaded memories into $DB"

# Score queries. Prints "i<TAB>rank_or_miss_or_adv<TAB>query".
score_queries() {
python3 - "$FIXTURES" <<'PY'
import json, subprocess, sys
fx = json.load(open(sys.argv[1]))
for i, q in enumerate(fx["queries"]):
    r = subprocess.run(["./target/debug/remem", "recall"] + q["query"].split()
                       + ["--k", "5", "--json", "--min-score", "0.017"],
                       capture_output=True, text=True)
    try:
        hits = json.loads(r.stdout)
    except json.JSONDecodeError:
        hits = []
    expect = q["expect"].lower()
    if not expect.strip():
        rank = "adv-pass" if not hits else "adv-fail"
    else:
        rank = "miss"
        for pos, h in enumerate(hits, 1):
            if expect in h["content"].lower():
                rank = pos
                break
    print(f"{i}\t{rank}\t{q['query']}")
PY
}
score_queries > /tmp/remem-eval-scores-$$.txt

total=$(awk -F'\t' '$2=="miss" || $2 ~ /^[0-9]+$/' /tmp/remem-eval-scores-$$.txt | wc -l)
r1=$(awk -F'\t' '$2==1' /tmp/remem-eval-scores-$$.txt | wc -l)
r5=$(awk -F'\t' '$2 ~ /^[0-9]+$/' /tmp/remem-eval-scores-$$.txt | wc -l)
adv_total=$(awk -F'\t' '$2 ~ /^adv-/' /tmp/remem-eval-scores-$$.txt | wc -l)
adv_pass=$(awk -F'\t' '$2=="adv-pass"' /tmp/remem-eval-scores-$$.txt | wc -l)

echo
printf "%-4s %-6s %s\n" "#" "rank" "query"
printf "%-4s %-6s %s\n" "----" "------" "-----"
while IFS=$'\t' read -r i rank q; do
    label=$rank; [ "$rank" = "miss" ] && label="MISS"
    printf "%-4s %-6s %s\n" "$((i+1))" "$label" "$q"
done < /tmp/remem-eval-scores-$$.txt

echo
echo "Queries: $total"
echo "recall@1: $r1/$total"
echo "recall@5: $r5/$total"
echo "adversarial: $adv_pass/$adv_total pass (empty result with --min-score 0.017)"
echo
echo "Misses:"
awk -F'\t' '$2=="miss" {print "  - " $3}' /tmp/remem-eval-scores-$$.txt
[ -s <(awk -F'\t' '$2=="miss"' /tmp/remem-eval-scores-$$.txt) ] || echo "  (none)"
echo
echo "Adversarial failures (returned hits instead of []):"
awk -F'\t' '$2=="adv-fail" {print "  - " $3}' /tmp/remem-eval-scores-$$.txt
[ -s <(awk -F'\t' '$2=="adv-fail"' /tmp/remem-eval-scores-$$.txt) ] || echo "  (none)"
