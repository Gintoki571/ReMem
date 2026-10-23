# Text-only knowledge extraction (track C1)

Scope: entities, relations, negation and validity dates from plain-text memories
(`remember` with no agent-supplied entities or relations). Branch `lane-nc-x-extract`.

## Benchmark

- `eval/extract/gold.json`: 128 labelled memories (48 from the first draft, 80 new).
  Covers the catalog failures: sentence-initial names ("Alicia lives in Jaipur."),
  titles with periods ("Dr. Patel"), "Owner:" prefixes, pronoun and "I"/"the user"
  subjects, absolute dates, negation ("no longer", "doesn't", "left", "never worked"),
  planned and recurring text, Spanish/French/German/Japanese, versions ("Node 20.11").
- The first draft was 120 rows, but rows 49 to 119 repeated the rows 15 to 47 pattern
  with new names. Those repeats were dropped because they inflated the rules tier.
- Scorer: `cargo run --release --example extract_eval -- eval/extract/gold.json [predictions.json] [name]`.
  Entity and relation scores are set-based and case-insensitive. Predicates map to
  `live | work | prefer | report | …` (coarse canonicalisation). Date accuracy counts only
  memories with an absolute gold `valid_from`. Negation accuracy counts only gold relations
  the system matched. Relative dates are not scored, because the rules tier uses the system clock.

## Comparison (128 memories)

| System | Entity P / R / F1 | Relation P / R / F1 | Date acc | Negation acc | Latency per memory (warm) | Memory | Size |
|---|---|---|---|---|---|---|---|
| Heuristic baseline (`src/extract.rs`, current) | 0.924 / 0.532 / 0.675 | 0 / 0 / 0 | 0.000 (none) | 1.000 (n/a) | ~0.01 ms | in-process | 0 |
| Rules tier (`src/extract_model.rs`, Rust) | 0.874 / 0.808 / **0.840** | 0.786 / 0.559 / **0.653** | 0.531 | 1.000 | ~0.03 ms | in-process | 0 |
| GLiNER2.5-small, Python + torch (CPU) | 0.837 / 0.880 / **0.858** | 0.615 / 0.814 / **0.701** | 0.000 (no dates) | 0.854 | 186 ms mean, 188 ms median (cold first 620 ms) | 1.47 GB peak RSS, of which most is torch | 296 MB safetensors (74M params, fp32) |
| Qwen2.5-1.5B-Instruct Q4_K_M, llama.cpp, grammar-constrained JSON | not run on 128 (see below) | | | | | | 1.12 GB GGUF |

### Same 40-memory slice (first 40 of gold)

| System | Entity F1 | Relation P / R / F1 | Date acc | Warm latency per memory | Peak RSS |
|---|---|---|---|---|---|
| Heuristic baseline | 0.593 | 0 / 0 / 0 | 0.000 | ~0.01 ms | in-process |
| Rules tier | 0.846 | 0.852 / 0.622 / **0.719** | 0.556 | ~0.02 ms | in-process |
| GLiNER2.5-small (Python) | 0.818 | 0.628 / 0.730 / **0.675** | 0.000 | ~0.19 s | 1.47 GB |
| Qwen2.5-1.5B Q4_K_M (llama.cpp) | 0.404 (P 1.000, R 0.253) | 0.625 / 0.541 / 0.580 | 0.556 | median 9.96 s, mean 16.6 s, cold first 6.5 s | 1.89 GB |

Qwen ran on 40 of 128 memories only, because a full run was too slow for this session.
Its precision is high but recall is low: many memories come back with no entities and no
relations, for example "Ivan no longer works at Acme." Model load was 8.8 s.

### Measurement setup

- CPU: 12 logical cores, 15 GB RAM, no GPU. Single process per system.
- GLiNER: `gliner2` 2.1.0 in `lanes/tmp/gliner-venv`, `AutoExtractor.from_pretrained(<local dir>)`,
  `extract_entities` with 9 generic types plus `extract_relations` with 14 predicate labels.
  Label lists were chosen from the gold predicates, so this is not a zero-shot test.
  Load 37.6 s. Scripts: `eval/extract/reference/run_gliner.py`.
- Qwen: `llama-cpp-python 0.3.16` (built from source), n_ctx 2048, `response_format` JSON schema (grammar),
  temperature 0, max_tokens 160. Scripts: `eval/extract/reference/run_llm.py`.
- Predictions: `eval/extract/preds/`. Re-score with
  `cargo run --release --example extract_eval -- eval/extract/gold.json eval/extract/preds/gliner2.5-small.json`.

## Findings

1. The baseline finds 53% of gold entities and no relations. Negation and dates are not extracted.
2. The rules tier takes relation F1 from 0 to 0.65 with no model and about 30 µs per memory.
   It is narrow: it only knows about 16 verb patterns and a few subject shapes. Its gold set was
   written by the same team, so these numbers are optimistic. A held-out set is still needed.
3. GLiNER2.5-small is the best quality measured. It is about 4 to 5 points better on relation F1
   than the rules tier, with no hand-written patterns. Its weaknesses are no dates, no negation,
   and relations that depend on the label list. The 0.19 s per memory is far too slow for
   a synchronous write path in Python, but reasonable for a background pass.
4. The 1.5B LLM with a strict grammar was the worst case for latency and entity recall in this
   setup. A 3B or larger model may do better, but none was measured here.

## Candle port plan for GLiNER2.5-small

Checkpoint facts (`gliner2.5-small-v1/config.json`, `model.safetensors`, 334 tensors, 296 MB, fp32):

- Encoder `encoder.*`: DeBERTa-v2 (`deberta-v3-xsmall`). 12 layers, hidden 384, 6 heads, FFN 1536,
  GELU, vocab 128,011, `pos_att_type` = c2p + p2c, `share_att_key`, 256 relative position buckets.
  `candle-transformers` 0.11 ships `models/debertav2.rs` (1,444 lines), which covers these keys. Reuse
  it after confirming the weight prefixes match. Start with a parity test on one sentence.
- Boundary head `boundary_head.*` (about 90 tensors): windowed boundary encoder (2 layers, 4 heads,
  window 128), query head, sparse start/end proposer (`end_top_k` 24, `ends_per_start` 12,
  `end_block_size` 256), shared candidate pool (budget 192), pair scorer, count head, and
  a null projection for abstention (threshold 0.5). Rotary endpoints are enabled.
- Relation scorer `relation_scorer.*`: MLP, content projections for head and tail, directional
  relation states, relation-content gate.
- Not needed for entities and relations: `classifier.*`, `record_decoder.*`.
- Python to mirror for inference (approximate line counts): `models/boundary/model.py` (2,258, mostly
  training code, so inference is a fraction), `engine.py` (1,542), `proposal.py` (751), `pool.py` (601),
  `relations.py` (419), `layers.py` (559), `processor.py` and `boundary_preprocessing.py` (about 2,400).
  Schema-to-input prompt construction lives in `processor.py` and must match exactly for parity.

Ops required, all available in candle 0.11 on CPU: embedding gather, LayerNorm, GELU, matmul/linear,
softmax, masked (windowed) attention, relative-position gather, top-k (via sort), sigmoid,
index_select/gather, outer-product pair scores, threshold and argmax, and rotary embeddings.
No custom kernels are expected. Span-to-character offsets come from the `tokenizers` crate, which
the project already uses.

Effort (single engineer, rough, ±50%):

| Step | Estimate |
|---|---|
| Encoder via `debertav2.rs`, weight loading, one-sentence parity vs Python | 2 to 4 days |
| Prompt builder and tokenizer offsets, parity on the schema prompt | 3 to 5 days |
| Boundary head, proposer, pool, decode (entities only) | 2 to 3 weeks |
| Relation scorer and decode | 1 week |
| Parity suite over gold (entity and relation sets equal Python output) | 3 to 5 days |
| Total | about 4 to 6 engineer-weeks |

Expected RAM: about 300 MB for fp32 weights, or about 150 MB in fp16. Latency is not measured in Rust yet.
Candle's CPU path may beat Python's 186 ms per memory, but that is unproven.

Dependencies: no new runtime service. `candle-transformers` would be an added crate, which needs
a one-line justification in the root AGENTS.md. Licence is Apache-2.0 for the checkpoint, per its README.

## Recommendation

1. Ship the rules tier only as an opt-in (`NEOCORTEX_EXTRACTOR=rules`, off by default). It is
   measurably better than the baseline on the gold set, has no dependencies, and is very fast. Before it
   becomes a default, write a held-out gold set of about 100 new memories written by someone else.
2. Do not ship the Qwen 1.5B path. It is too slow (about 10 s per memory) and recalls too few entities.
3. Take GLiNER2.5-small to a spike rather than a default: port the encoder, prompt builder and
   entity-only boundary path to candle behind the same flag, and check parity against the Python
   dump for the 128 memories. Only then decide on relations. The entity-only spike is about 3 to 4
   weeks; the full port is about 4 to 6 weeks (see the effort table).
4. Use GLiNER as a background enrichment pass in the first version, not on the synchronous write
   path. Its 0.19 s per memory in Python is too slow for a write that must return at once.

## Risks and gaps

- The gold set and the rules tier were written by the same team, so the rules numbers are optimistic.
  GLiNER's label list was chosen from the gold predicates, which is also optimistic.
- Date and negation numbers are weak. Relative dates ("next spring", "last March") are not scored.
  The rules tier now takes the engine clock for them, but no gold labels check it yet.
- Relation predicates are scored with a coarse canonical map (`live`, `work`, `prefer`, `report`).
- Python numbers include torch overhead; the RSS figure is not a Rust figure.
- The Qwen run covers 40 memories only.
- `jev` and `laya` (the decision models mentioned in the brief) were not evaluated in this track.
- The test suite opens the default DB through `neocortex eval` when `NEOCORTEX_DB` is unset
  (`tests/cli.rs` and `cmd_eval` in `src/main.rs`). Run tests with `NEOCORTEX_DB` pointed at a
  scratch file. That is a separate fix, outside this lane's files.

## Held-out check (2026-10-23)

`eval/extract/holdout.json`: 150 memories written independently (the author did not see the rules tier, the first gold set or this document). Scored with `cargo run --release --example extract_eval -- eval/extract/holdout.json [preds.json]`:

| system | entity F1 | relation F1 | date acc | negation acc |
|---|---|---|---|---|
| heuristic | 0.698 | 0.000 | 0.000 | 1.000 |
| rules tier | 0.682 | 0.066 | 0.765 | 0.833 |
| GLiNER2.5-small (Python, same label lists) | 0.707 | 0.200 | – | 0.783 |

The rules tier's 0.62 relation F1 on `gold.json` was overfitting to its author's phrasing. No CPU-sized extractor measured here (rules, GLiNER2.5-small, Qwen2.5-1.5B) extracts relations from free text reliably. Decision: the calling agent (an LLM) is the extractor of record; neocortex keeps the rules tier opt-in, nudges agents to pass `entities`/`relations`, and treats a GLiNER port as optional background enrichment, not a write-path dependency.
