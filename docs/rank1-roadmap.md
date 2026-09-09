# Rank-1 roadmap: Q27 + Q28 (offline analysis, no code changed)

Method: debug binary at `bcc0a14`, fresh scratch DB `/tmp/rank1-scratch.db`
(40 memories via `remem remember`, Cpu 768d), `remem recall --k 5 --json
--min-score 0`. Full sweep: 35/37 @1; the ONLY non-@1 answerable queries
are Q27 (rank 3) and Q28 (rank 2). Adversarial 3/3 unaffected (not re-measured
here; floor unchanged by anything proposed).

## Top-5 dumps

### Q27: "wait what did we decide at the start of the year about spending" (expect `Q1 decision`)

| pos | score | reasons | content (trunc) | tags |
|-----|-------|---------|-----------------|------|
| 1 | 0.04615 | fts#1, vector#6, recent | Agreed to standardize on OpenTelemetry for all new services… | monitoring, decision |
| 2 | 0.04593 | fts#3, vector#2, recent | We chose SQLite over Postgres for the local cache… | decision, storage |
| 3 | 0.04470 | fts#5, vector#1, recent | The Q1 decision was to freeze new infra purchases… (TARGET) | decision, process |
| 4 | 0.04407 | fts#2, vector#9, recent | Onboarding docs live in the wiki… | docs, onboarding |
| 5 | 0.04052 | fts#4, vector#15, recent | Q2 decision: migrate the analytics warehouse… | decision, analytics |

Rank-1 distractor (OpenTelemetry) has what the target lacks: an
exact/stemmed high-BM25 match on `start` ("starting … next quarter"),
ranking fts#1. The target's only lexical hook is the `deci*` prefix arm
(query `decided` stems `decid`, memory `decision` stems `decis`), which
lands it at fts#5 — worst of the five — while it wins vector#1 outright.

### Q28: "did the auditors ever get back to us about that winter check" (expect `February 20`)

| pos | score | reasons | content (trunc) | tags |
|-----|-------|---------|-----------------|------|
| 1 | 0.04577 | fts#1, vector#7, recent, important | Keeping the staging DB credential inside a checked-in config… | security, bug |
| 2 | 0.04554 | fts#4, vector#1, recent | Quarterly security audit wrapped on February 20… (TARGET) | security, process |
| 3 | 0.04372 | fts#5, vector#3, recent | Rotated the API signing keys after the audit finding… | security, api |
| 4 | 0.04236 | fts#2, vector#15, recent | Onboarding docs live in the wiki… | docs, onboarding |
| 5 | 0.03030 | fts#3, recent (no vector) | We use Rust for the ingest pipeline… | perf, pipeline |

Rank-1 distractor (staging credential) has what the target lacks: an exact
high-IDF token match on `check` ("checked-in" stems to `check`), ranking
fts#1, plus the `important` reason (importance 0.8 vs target 0.7). The
target's only lexical hook is the `audi*` prefix arm (query `auditors` ->
`auditor` vs memory `audit`), landing fts#4 while winning vector#1 outright.

## Classification

| query | class | rationale |
|-------|-------|-----------|
| Q27 | winnable-by-ranking (FTS-vs-vector balance) | Target already vector#1; both signals present, fusion weights the fts#1/vector#6 distractor over the fts#5/vector#1 target. No content or semantic gap. |
| Q28 | winnable-by-ranking (FTS-vs-vector balance) | Same shape: target vector#1, loses to fts#1/vector#7 + importance flag. No content or semantic gap. |

Neither is needs-content (both memories already contain the answer text and
win the vector channel) nor needs-semantics (no paraphrase beyond
lexical+vector is required — the embedder already ranks both targets #1).

Common mechanism (new since the hybrid-prefix landing `e3f1da9`): both
targets match FTS ONLY via the 4-char prefix arm, while their distractors
match via exact/stemmed terms. The OR-query scores arm-only and exact
matches on one BM25 scale, so an exact match on a common token (`start`,
`check`) outranks the target's sole discriminating hook. The weight-spike
conclusion (`docs/weight-spike.md`: FTS weight >= vector wins) predates the
prefix arms and is stale for exactly these two queries.

## Recommended next experiment (one only)

Offline re-fusion sweep on the frozen `--k 40` reasons lists: discount
FTS hits that match ONLY via a prefix arm (equivalently: sweep vector
weight 0.5 -> 0.75/1.0 with arm-only FTS hits demoted one BM25 rank).
Predicted effect: Q27 fts#5 -> mid-list lets vector#1 decide => rank 1;
Q28 fts#4 -> mid-list lets vector#1 decide => rank 1; 37/37 @1 with no
other movement (all other 35 targets already win both channels, so a
demotion that only touches arm-only hits cannot displace them).
Re-measure adversarial junk in the same run (arm discount must not lift
Q39 `work*`-class hits; gate predicts 0 change, verify live).
Smallest change if the sweep confirms: one rank-adjustment at the FTS call
site, no schema/index rebuild, no weight retune.
