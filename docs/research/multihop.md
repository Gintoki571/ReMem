# Multi-hop, aggregation, negation and abstention (track C2)

Status: implemented on `lane-nc-x-multihop` (v0.1.4). Default-on: query-directed
path channel plus rendered path facts. New tools: `follow`, `aggregate`.
Negation filtering and a calibrated abstention gate are on by default.

## 1. What was broken

Baseline at `d6448fc` on the committed 300-person world
(`eval/world-300.json`, 3 264 memories, 600 queries), measured with the fork
binary on the saved DB (`neocortex eval eval/world-300.json --load-db`):

| family | acc@10 | recall | MRR |
| --- | --- | --- | --- |
| two-hop (120) | 0.008 | 0.358 | 0.616 |
| three-hop (78) | 0.000 | 0.333 | 0.693 |
| current / as-of / history / window / aggregation | 1.000 | >= 0.98 | >= 0.82 |

The task brief's 1 000-person numbers (two-hop 0.033, three-hop 0.013) reproduce
the same shape at larger scale.

### Root cause

The question "which city does {person}'s manager live in" names one entity. The
answer memory ("{manager} lives in {city}") shares **no word** with the
question. Every existing channel is neighbourhood retrieval around the named
entity:

* BM25 only matches the question words;
* the dense channel matches paraphrase, not the two-hop chain;
* PPR spreads from the seed entity and does return the manager — but the
  manager's *own* residence memory needs another hop and ranks far below the
  first 10;
* the rendered facts were built from entities adjacent to the returned
  memories, so the last hop (manager -> city) was never rendered.

So recall named the neighbourhood and not the answer. Two earlier defects made
a fix harder than it looked:

* `text::singularize("lives") == "life"` (the `-ives` -> `-ife` rule), so naive
  lexical matching of "live" against a "lives in" edge fails. Matching now uses
  a form set per token: raw, possessive-stripped, `singularize`, and
  `text::norm_predicate` (`lives` -> `live`).
* The world writes the same predicate with different surface forms
  ("Elsa Roynath's home is in Melbourne" vs "Liam Vijappa lives in Denver"), so
  a query term "home" must also match the "live" vocabulary. A small question
  vocabulary maps `home`/`live`, `job`/`work`, `boss`/`manager`,
  `sister`/`brother`/`sibling`, `language`/`use`, `like`/`prefer`.

## 2. Ideas considered

### 2.1 LLM-free query-directed path beam search — chosen

This is the LLM-free shape of Think-on-Graph (Sun et al., 2023) / RoG (Luo et
al., 2024): instead of an LLM choosing which relation to expand, the question's
own content words choose. `src/paths.rs`:

1. Seeds: entities named exactly in the question (alias lookup), never fuzzy
   vector seeds — those start paths from unrelated entities.
2. Terms: question words minus stopwords, answer-type words (`city`,
   `language`, `count`, ...: the answer type cannot discriminate paths) and
   anchor-name tokens, expanded through the synonym table above, capped at 16.
3. Beam (24) breadth-first expansion to depth 3 over relations whose predicate
   or fact matches at least one term. Every hop must add a **new** question
   term, so a chain only survives if it explains more of the question.
   Reverse traversal (`dst -> src`, e.g. "X's manager" stored as
   `manager -> X`) is allowed at 0.75 quality.
4. Rank by coverage first, then edge quality, then shorter chain. Coverage is
   what separates a 3-hop chain that explains the whole question from a 2-hop
   fragment with a better edge score.
5. Evidence: destination-hop memories get 3x weight, scaled by
   `(1 + coverage)^2`; they feed an RRF channel (`path`, weight 2.5).
6. The relations along the best two chains are rendered as facts, destination
   hop first. This is what makes acc@10 jump: the answer often exists only as
   "Liam Vijappa —live in→ Denver", on an entity the neighbourhood search never
   reached.

### 2.2 Query decomposition with a small LLM — not run, rejected

The benchmark's questions are templated, and rule-based decomposition (terms +
graph walk) already reaches 1.00 acc@10. A 4-bit <= 3B model on candle would
add model weights, prompt latency (hundreds of ms per query on CPU) and a
failure mode (bad decompositions) for no accuracy left to win on this data.
The no-network, candle-only constraint also rules out pulling a model at run
time.

### 2.3 Tiny trained "System-1" scorer (SubgraphRAG shape) — not run, deferred

SubgraphRAG/GNN-RAG-style scorers rank candidate triples with a small trained
model. Training data can be generated from a *different* `gen_world` seed (never
the evaluation world), which is legitimate. It was not built because (a) the
lexical beam already saturates these benchmarks (on world-300 three-hop acc@10
is 1.000 and the five remaining two-hop misses are answered by one `follow`
call, so a better triple ranker has no headroom on the measured metric), and
(b) it introduces a training artifact and a model file
into a crate that currently has none. It remains the right next step for
open-domain multi-hop, where question words do not overlap the relation
vocabulary; there it would be measured against paraphrased-relation queries,
which this corpus does not contain.

### 2.4 Agent-facing tools — kept (`follow`, `aggregate`)

* `follow(from, to?, predicate?, depth<=3)`: walk live relations hop by hop,
  with direction, hop count and evidence memory ids. This is the tool an agent
  uses to finish a chain itself.
* `aggregate(predicate?, source?, target?, include_history?, limit?)`: count
  and list distinct entities reaching one end of a relation, and return
  negated facts separately under `negative`. Ranked recall cannot answer
  "how many / which ones"; this can.

Both are exposed over MCP (10 tools now) and tested end to end.
`timeline` did not need a new tool: `entity` already returns the mentioning
memories newest first plus `current_facts` / `past_facts`.

### 2.5 Negation-aware filtering — kept, simple

`paths::negation_intent` folds the question and looks for a negation cue
(`not`, `no`, `never`, `without`, `none`, ...). When present:

* the path channel is skipped (a path would surface the positive side);
* memories and facts are filtered to rows whose own text states a negative
  (`paths::has_negation_cue`), and the answer abstains if nothing states one.

On the catalogue repro (5 "likes jazz" facts + "Bob does not like jazz",
question "who does not like jazz") recall now returns only Bob. This is
text-level: it does not reason about absence ("who does *not* work here" is not
answered by absence evidence). That is the honest limit of a no-LLM design.

### 2.6 Abstention — kept, conservative and calibrated

When no channel found anything to stand on (no BM25 hit, no entity seed, no
relation seed, no path, no time window) and the best dense cosine is below
`ABSTAIN_COSINE`, recall returns an empty result instead of the nearest
unrelated memories.

Calibration is the weak part. On UltraBERT-150M the *stored but paraphrased*
and *off-domain* distributions overlap heavily (holdout queries with no other
support: 0.185, 0.211, 0.252, 0.256; off-domain probes: 0.120, 0.133, 0.180,
0.263, 0.273, 0.313). The threshold is 0.18: it abstains on clearly unrelated
questions ("how do I bake sourdough", "who won the 1998 world cup") and keeps
every holdout query. A larger, better-calibrated signal needs a score with a
real margin (e.g. a trained verifier), not a raw cosine.

### 2.7 "jev" and "laya" decision models

* **Jev** is a proprietary, hosted decision model (API only, no public
  weights). Under a no-network, local-first runtime it cannot be embedded, and
  its interface (choose an action) does not retrieve facts.
* **Laya** is an open-weight ModernBERT-large (421M parameter) decision model
  shipped for Python/ONNX; reported 193-464 ms per request on CPU, and it is a
  reranker/choice model, not a retriever. That is a heavier runtime than the
  whole 150M embedder for a component that still needs a candidate generator.
  Not adopted.
* JEPA-family representation models (I-JEPA, LeJEPA, V-JEPA 2) are pretraining
  objectives, not retrieval or decision components. Not applicable.

### 2.8 GraphRAG landscape (Awesome-GraphRAG scan)

* HippoRAG-style personalized PageRank: already in recall as the `graph`
  channel.
* Think-on-Graph / RoG: LLM beam over relations -> emulated without an LLM
  here (2.1).
* PathRAG / SubgraphRAG / GNN-RAG: retrieve paths/subgraphs then prune with a
  learned or heuristic scorer -> path beam + coverage ranking is the
  no-training version.
* Community summaries: already present as `overview`.
* StructGPT: tool-augmented traversal for LLMs -> the `follow`/`aggregate`
  tools play this role for an agent.
* Local projects (same machine): the `main`-branch worktree of this crate
  (`remem-v31`) has no relation-path retrieval, only a neighbourhood test; LightRAG extracts
  low/high-level keywords and retrieves at both levels (its keyword split is
  the same seed/term split used here, its LLM extraction is out of scope);
  graphify exposes `path A B` shortest-path queries with
  EXTRACTED/INFERRED edge confidence (the shape of `follow`); agentmemory fuses
  BM25 with structural graph matches, i.e. the RRF design already in recall.

## 3. Results

All rows: same machine, CPU only, `--load-db` from an ingested DB. "subset"
means `--limit-queries 300` (first 300 queries, all families mixed).

### 3.1 World-300, 600 queries

| variant | acc@10 | recall | MRR | two-hop acc / recall | three-hop acc / recall | latency mean / p50 / p95 |
| --- | --- | --- | --- | --- | --- | --- |
| baseline (`d6448fc`) | 0.672 | 0.782 | 0.821 | 0.008 / 0.358 | 0.000 / 0.333 | 253 / 102 / 289 ms |
| path channel only, no synonyms/facts (subset) | 0.813 | 0.871 | 0.866 | 0.651 / 0.605 | 0.047 / 0.496 | 236 / 104 / 958 ms* |
| path channel + path facts (subset) | 0.997 | - | 0.867 | 0.977 / 0.628 | 1.000 / 0.411 | 124 / 110 / 175 ms |
| final (synonyms + facts, weight 1.5) | 0.992 | 0.847 | 0.869 | 0.958 / 0.633 | 1.000 / 0.389 | 139 / 118 / 205 ms |
| final, weight 1.0 | 0.992 | 0.812 | 0.858 | 0.958 / 0.492 | 1.000 / 0.333 | 109 / 100 / 118 ms |
| final, weight 0.75 | 0.992 | 0.802 | 0.853 | 0.958 / 0.446 | 1.000 / 0.333 | 127 / 113 / 145 ms |
| **final, weight 2.5 (default)** | **0.992** | **0.991** | **0.882** | **0.958 / 0.979** | **1.000 / 0.962** | **107 / 101 / 124 ms** |

\* the 958 ms p95 was a cold-page-cache outlier on the first run after a fresh
ingest; repeated runs of the same build were 175-205 ms.

Per family at the default (world-300): current 1.000/1.000/0.893,
as-of 1.000/1.000/0.822, history 1.000/1.000/0.994, window 1.000/1.000/1.000,
aggregation 1.000/1.000/0.992, two-hop 0.958/0.979/0.795,
three-hop 1.000/0.962/0.797 (acc@10 / recall / MRR).

### 3.1b World-1000 confirmation (10 822 memories, same 600 query templates)

`cargo run --release --example gen_world -- --people 1000 --seed 7`, then both
binaries against the same saved DB (`--load-db`), 12-core machine also running
another lane's bench:

| variant | acc@10 | recall | MRR | two-hop acc / recall | three-hop acc / recall | latency mean / p50 / p95 |
| --- | --- | --- | --- | --- | --- | --- |
| baseline (`d6448fc`) | 0.678 | 0.763 | 0.786 | 0.025 / 0.325 | 0.026 / 0.333 | 210 / 162 / 376 ms |
| **final** | **0.995** | **0.983** | **0.855** | **0.975 / 0.988** | **1.000 / 0.936** | **171 / 146 / 264 ms** |

The brief's baseline (two-hop 0.033, three-hop 0.013) reproduces with the same
shape: 0.025 and 0.026 (differences from the exact world build/seed). Every
other family is unchanged or better; three-hop MRR is lower than on world-300
(0.704) because with 1 000 people more distractor memories outrank the same
evidence, while acc@10 stays at 1.000.

### 3.2 Fixtures and holdout (gate)

| set | baseline recall / MRR | new recall / MRR |
| --- | --- | --- |
| `eval/fixtures.json` (95 / 84) | 0.953 / 0.894 | 0.953 / 0.900 |
| `eval/holdout.json` (105 / 88) | 0.924 / 0.794 | 0.924 / 0.794 |

Both sets are unchanged family by family; the only delta is fixtures `as-of`
MRR 0.929 -> 1.000. Regression bound (-0.005) holds with room to spare.

Two intermediate states did hurt holdout and were fixed rather than accepted:

* abstention at cosine 0.45 zeroed two stored-but-paraphrased queries
  (holdout recall 0.913) -> threshold 0.18;
* the path channel on history-intent questions boosted the *current* state
  above the older one, costing 0.011 holdout MRR -> path search is skipped when
  the question says "before"/"used to" (the existing history boost already
  handles those).

### 3.3 Agent in the loop

`examples/agent_sim.rs` simulates an agent with two tools:

1. `recall` the question (`k=10`, the same context width eval scores);
2. if any `answers` string is still missing, extract the longest word run in the
   question that exactly names an entity and `follow` it three hops;
3. if still missing, `recall` again with the deepest reached entity's name.

World-300, 600 queries: `recall` alone answers 595/600 (0.992, identical to the
eval acc@10), and the five misses are all two-hop. `follow` rescues all five,
so the two-call agent answers 600/600 (1.000); calls per query 1.01, `follow`
p50 4 ms, `recall` p50 107 ms.

World-1000, same 600 queries: `recall` alone 597/600 (0.995), `follow` rescues
the three two-hop misses -> 600/600 (1.000); calls per query 1.00, `follow`
p50 8 ms, `recall` p50 154 ms.

The simulation caught a real tool defect at 1 000 people. Walking from a person
reaches a hub (a city, a tool, a project's language) whose hundreds of reverse
edges ("X prefers Zig", "X lives in Nairobi") filled `k` before the informative
forward hop ("Nimbus Prism uses Scala"), so the agent could not finish the
chain. Fixed by ordering facts at the same hop forward-edge-first
(`src/paths.rs`), with regression test
`follow_prefers_forward_edges_over_hub_noise`. This changes only `follow`; the
eval and recall numbers above are unaffected.

## 4. Cost

* No new crates, no model files, no new runtime.
* Path search cost is bounded: <= 4 seeds, beam 24, 3 hops, 12 relations per
  entity, per-entity memoised. On world-300 the end-to-end p50 is unchanged
  (101 ms vs 102 ms baseline) and p95 is lower (124 ms vs 289 ms) because the
  answer memory renders earlier and packing drops less. On world-1000 the new
  build is also faster than the baseline it replaces (p50 146 vs 162 ms, p95
  264 vs 376 ms) while doing strictly more work.
* `aggregate` is two indexed scans; `follow` is a depth-3 BFS with one query
  per visited entity.

## 5. Recommendation

* Keep the path channel and path facts **on by default** (weight 2.5): the only
  change that moves two/three-hop from ~0 to ~1.0 without touching any other
  family.
* Keep `follow` and `aggregate` as agent-facing tools. They are the honest
  fallback when a chain has no lexical overlap with the question, and they make
  aggregation a first-class operation.
* Keep negation filtering text-level and abstention conservative; both are
  correct where they fire, and the abstention threshold is at the edge of what
  a raw cosine can separate.
* Next steps, in order of expected value: (1) a trained triple scorer on
  generated worlds to cover paraphrased relations; (2) an abstention score with
  margin (top-1 vs top-k gap, or a tiny verifier) instead of a raw cosine;
  (3) absence-aware negation ("who does not work here") backed by an explicit
  "not" fact in storage, which this branch's schema does not yet record.

## 6. Independent check and the semantic beam (2026-10-23, orchestrator)

The lexical beam's world numbers lean on a synonym table written for the world generator's vocabulary. `eval/multihop-holdout.json` was written by an agent that never saw `src/paths.rs`, `gen_world.rs` or this document: 155 memories, 87 distinct predicates, 80 queries (40 two-hop, 25 three-hop, 15 single-hop) phrased with paraphrase and descriptions ("the engineer who guides Noor's early career" for `mentors`).

| build | two-hop acc@10 | three-hop acc@10 | fixtures recall/MRR | holdout recall/MRR |
|---|---|---|---|---|
| main (no path search) | 0.775 | 0.560 | 0.953 / 0.894 | 0.924 / 0.794 |
| lexical beam only | 0.775 | 0.600 | 0.953 / 0.900 | 0.924 / 0.794 |
| + semantic beam, w_sempath 0.25 (default) | **0.850** | **0.720** | 0.951 / 0.902 | 0.924 / 0.795 |

The lexical beam barely fires there: questions paraphrase predicates and name people by first name ("Noor's") while path starts were exact aliases only. `paths::search_semantic` fixes both: one KNN (k=100) over `relations_vec` picks the facts the question recognises, the beam starts from the exact anchors plus the top fuzzy entity seeds, walks only recognised facts (6 per entity, depth 3), scores each hop against the *residual* question (question vector minus the components earlier hop facts explained), sums hop scores, and emits only >= 2-hop chains into their own RRF channel `sem` (weight `w_sempath`). Measured path scores on the independent set: mean cosine 0.775/0.640, sum 0.850/0.640, residual sum 0.875/0.720 (two/three-hop). A weight of 1.0 cost 0.014-0.04 fixtures recall and collapsed world aggregation/as-of MRR (0.99 -> 0.39); 0.25 keeps every other family. World-300 at the default: two-hop 0.958, three-hop 1.000, all other families 1.000, p50 93 ms.

Caveat: `w_sempath` was chosen on this set (4 values, all sets agreeing), so it is no longer fully independent; a second independent multi-hop set is the next honest check.

## 7. Second independent set, homonym merges and prose hops (2026-10-23, orchestrator)

`eval/multihop-holdout2.json` was written by an agent forbidden to read `src/`, `examples/`, `docs/research/` and the other multi-hop sets: 220 memories (25% text-only prose with no entities or relations passed), 112 distinct predicates, 100 queries (40 two-hop, 30 three-hop, 15 updated-chain, 15 distractor). Nothing was tuned on it before the first measurement.

| build | acc@10 | two-hop | three-hop | updated-chain | distractor |
|---|---|---|---|---|---|
| main 261dfb0 (first, untouched measurement) | **0.720** | 0.750 | 0.500 | 0.867 | 0.933 |
| + F2 + alias guard | 0.720 | 0.800 | 0.467 | 0.800 | 0.933 |
| + prose hops + anchor pass-through block (PROSE_HOP 0.8) | **0.820** | 0.875 | 0.667 | 0.867 | 0.933 |

What the misses were (main):
* 22 of 38 missing chain memories were text-only (25% of the set): the beams walked relations only, so a link stated in prose was unreachable.
* Three homonym pairs were merged into one entity: "Teresa Mota" (alias "Teresa") resolved to "Teresa Moura" through the shared short alias. Fixed in `resolve::exact_match`: an alias match needs the alias to be the entity's own name or compatible full names (word subset; words with digits ignored so dose/version variants still merge).
* Fuzzy seeds walked back through the question's anchor and outranked the real chain with sibling facts ("Ivo designed Tidewater | Tidewater was founded by Inês"). Fixed: a semantic chain may start at an exact anchor but never pass through one.

Prose hops (`search_semantic`): a text-only memory (no relation evidence) among the recall vector channel's top-100 neighbours is a hop between the entities it mentions, or a terminal hop when it names nobody else; it is scored by its own vector times `PROSE_HOP` (0.8) and only with an exact anchor in the question. Its evidence is the memory itself (`Step.memory`).

Latency: reading a vector out of vec0 costs ~0.3 ms, so a second k=100 KNN returning embeddings added 38 ms per recall on world-300. The beam now reuses the vector channel's neighbour ids and reads vectors lazily, only for edges it reaches (relation facts too, which the first semantic beam also read eagerly). Same 200 world-300 queries, same session: main 261dfb0 125 ms p50, F2 131 ms, eager prose hops 170 ms, final 122 ms. Absolute numbers drift with machine state (full world-300 runs measured 81-87 ms earlier in the day, 114 ms at the final gate); compare within one session.

Rejected: reserving an output slot for the best prose-ending chain's memory (its answer renders nowhere else) gave +0.01 on this set and fixtures but cost a holdout query (recall 0.913 -> 0.901).

Trade-offs, measured: holdout recall 0.924 -> 0.913 is one query ("the boat she takes out on the river": the expect_absent touring-kayak memory now outranks the single scull, reached through Ingrid's prose memories); multihop-holdout two-hop 0.850 -> 0.825 is one query ("partner of Ravi Menon": "resides in Antwerp | Seun born in Antwerp" outscores "married to | born in Naples" because "partner" barely matches "married to"). Both are the paraphrase weakness a trained triple scorer would address. Variants that fixed the boat query (memory hops only after a relation hop; anchor-only seeds) cost more elsewhere (fixtures recall 0.941; holdout MRR 0.778).

Remaining misses are three-hop chains whose middle hop is a paraphrase ("partner"/"married to", "trained"/"studied at") or whose answer sits in a long prose memory. Single-shot retrieval is at 0.82 on this set; the >90% bar needs either the agent loop (`follow` on the bridge entity, measured 600/600 on world-1000 in 3.3) or a learned hop scorer.
