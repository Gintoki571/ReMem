# Temporal eval: does recency earn its keep when the time spread is real?

`docs/ranking-study.md` scored pure 1:1 RRF (34/37) against fused-with-multipliers
(31/37) on fixtures written moments ago, so every row had recency ~1.0 and the
recency multiplier had nothing to say. This run repeats the comparison with TRUE
backdated event times. Testing and docs only; no code changed.

## Setup

- Binary: `./target/debug/remem`, branch `v3` (HEAD clean apart from
  `docs/diagrams/*`). Embedder: Cpu 768d BERT. Clock at run time: 2026-09-09 ~00:40 UTC.
- Scratch DBs under `/tmp`, one arm per file, built from the same 17 rows:
  the 7 date-sensitive `docs/eval-fixtures.json` memories with real
  `--occurred-at` backdating, plus 10 regular memories stored un-backdated.
  - `march outage` -> 2026-03-15, `Q1 freeze` -> 2026-01-20, `Feb 20 audit` ->
    2026-02-20, `April billing` -> 2026-04-15, `BigQuery` -> 2026-05-01,
    `Aug pipeline` -> 2026-08-03, `v1 shutdown` -> 2026-12-31 (future-dated).
- No `--no-recency` flag exists, so recency is flattened externally rather than
  inferred from ranks. Three arms, all with identical FTS and vector input:

| arm | file | construction | what it isolates |
|-----|------|--------------|------------------|
| default | `/tmp/remem-temporal.db` | events backdated, 10 controls stored today (age 0) | shipping behaviour |
| recency-flat | `/tmp/temporal/flat.db` | `occurred_at` NULLed and `created_at`/`updated_at` pinned to one identical value, so every row has age 0 and band = 1.0 | RRF + importance, recency removed |
| all-old | `/tmp/temporal/spread.db` | as default, but the 10 controls pinned to 2026-06-01; events keep their own dates | whether the fresh-vs-old gap is doing the work |

- Soundness check, run on every query: the recency-flat arm reproduces the
  default arm's per-id `fts#N`/`vector#N` ranks exactly (17/17 ids, all 5
  queries), so any rank move between those arms is the recency band alone.
  Row counts verified after each write (`17` rows; `occurred_at` 7 vs 0;
  `count(distinct created_at)` 17 vs 1).

## Table (paraphrase queries, k=5)

| query | event age | recency band | rank default | rank in-window |
|-------|-----------|--------------|--------------|----------------|
| `march outage` | +178 d | 0.9016 | 3 | 1 (1 hit) |
| `q1 freeze decision` | +232 d | 0.9005 | 1 | 1 (1 hit) |
| `august maintenance` | +37 d | 0.9425 | 1 | 1 (1 hit) |
| `december shutdown` | -113 d (future, clamps to 1.0) | 1.0000 | 1 | 1 (1 hit) |
| `february audit` | +201 d | 0.9010 | 1 | 1 (1 hit) |

Wider columns for the same five queries (rank of the expected memory):

| query | default | recency-flat | all-old | `--since/--until` ±15 d |
|-------|---------|--------------|---------|--------------------------|
| `march outage` | 3 | 1 | 1 | 1 (1 of 17 rows survive) |
| `q1 freeze decision` | 1 | 1 | 1 | 1 |
| `august maintenance` | 1 | 1 | 1 | 1 |
| `december shutdown` | 1 | 1 | 1 | 1 |
| `february audit` | 1 | 1 | 1 | 1 |

The fixture's own adversarial rewordings of the same seven memories
(`docs/eval-fixtures.json` queries #26-#32) on the same 17 rows:

| # | query | default | recency-flat | all-old |
|---|-------|---------|--------------|---------|
| 26 | hey so remember when things went down earlier in the year, like that whole march thing | 1 | 1 | 1 |
| 27 | wait what did we decide at the start of the year about spending | 3 | 2 | 3 |
| 28 | did the auditors ever get back to us about that winter check | 4 | 1 | 2 |
| 29 | there was something with invoices being late a while back, when was that | 1 | 1 | 1 |
| 30 | what was the plan again for moving analytics somewhere else | 1 | 1 | 1 |
| 31 | how long was the pipeline down during that summer maintenance thing | 1 | 1 | 1 |
| 32 | when does the old public api go away exactly | 1 | 1 | 1 |

recall@1 default 5/7, recency-flat 6/7, all-old 5/7; recall@5 7/7 in every arm.
#27 loses rank with recency on and off (3 vs 2), so it is the Q1/Q2 collision,
not the clock. #28 is the one case recency demonstrably costs.
The 12 date-free control queries rank identically in the default and flat arms
(10/12 recall@1, the 2 misses being rows absent from this 17-memory subset), so
recency is not perturbing timeless recall either.

## What the numbers say

- The band is `0.9 + 0.1 * recency`, so recency can move a score by at most
  11.05% (fresh vs anything older than ~120 d). Inside the aged set the spread
  is 131-232 d -> 0.9005..0.9048, i.e. **0.49%**. Recency has no power to order
  old against old, and the all-old arm confirms it: `march outage` returns to
  rank 1 once the controls are aged out too, so the default arm's demotion is a
  fresh-vs-old effect, not the March event being intrinsically outranked.
- Adjacent dual-list fusion gaps are 1.64% (`fts#1+vector#1` vs
  `fts#2+vector#2`) and 3.28% (vs `#3+#3`). So the 11% swing *can* flip an
  adjacent pair, and a 3-row probe shows it does: a fresh `fts#2+vector#2` row
  (0.03244) beat a 247-day-old `fts#1+vector#1` row (0.02976), and with the
  dates swapped the `#1+#1` row keeps rank 1. Recency is inert neither way.
- It cannot beat agreement-vs-single-list: `fts#1+vector#1` vs `vector#1` alone
  is a 100% gap, far outside the band. That, not recency, is why the flat arm
  still trails pure 1:1 RRF.
- The `march outage` demotion is arithmetic, not noise: the answer sits 3.28%
  above `fts#3+vector#3` on fusion and 1.02% above on importance, so it needs
  4.33% net. Recency hands the fresh competitor +9.84%, landing the answer
  5.93% behind -> rank 3 instead of 1.
- Future-dated rows clamp to age 0 (`recency_score` uses `(now - t).max(0)`),
  so the 2026-12-31 deprecation notice scores as maximally recent. In the
  all-old arm that makes it the single row with band 1.0 and it takes slot 1 on
  query #28 for that reason alone. A scheduled future event is not "recent";
  it is being treated as such.
- The `--since/--until` window arm is the honest temporal control: it narrows
  17 rows to 1 for every date-anchored query. When the user names a time, an
  exact filter beats a soft multiplier by a wide margin, and it is explainable.

## Verdict

The 34/37 vs 31/37 result is **not** overturned by real time spread, and the
earlier reading that "recency had no signal to exploit" was also wrong. With
genuine spread recency does move ranks -- 15-17 of 17 rows shift position per query,
it flips adjacent fusion ranks, and it can rescue a fresher second-list row --
but on this corpus it buys 0 top-1s and costs exactly 1 (recall@1 6/7 flat vs
5/7 default; recall@5 7/7 both). It is a tie-breaker on a 0.49% band for the
aged rows it is supposed to arbitrate, which is what the narrow band was
designed to make it. Keep the band as it is; do not widen it, since widening to
override fusion is the failure `docs/multiplier-proposal.md` already measured.
For time-anchored queries the correct lever is `--since/--until`, not the
multiplier, and the caller that knows the date should pass it.

Two things this run cannot settle: 17 rows and 5 paraphrases is small, and the
demotion is phrasing-dependent (the same March event ranks 1 under the
fixture's verbose #26 and 3 under the terse `march outage`), so per-query rank
deltas here are weaker evidence than the band arithmetic above. And the
future-dated clamp is a real design question, not a measurement artifact.

Reproduce: build the three arms from one 17-row load (`/tmp/remem-temporal.db`),
then `cp` + the `UPDATE`s in the table above. `cp` alone can miss WAL content,
so verify `select count(*), count(occurred_at), count(distinct created_at) from
memories;` after each copy before trusting an arm.
