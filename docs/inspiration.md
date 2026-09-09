# Inspiration mining: hindsight + LightRAG -> ReMem v3

Sources read 2026-09-08: `/home/bindesh/rag/hindsight` (docs `versioned_docs/version-0.9/developer/*`, code `hindsight-api-slim/hindsight_api/engine/*`), `/home/bindesh/rag/lightrag` (`lightrag/operate.py`, `lightrag/prompt.py`, `docs/ProgramingWithCore.md`). `hindsight-api` is empty; the implementation lives in `hindsight-api-slim`.

v3 baseline for comparison: `crates/*/src/*.rs` (about 2.0k lines), one SQLite file, no LLM in the engine, `MemoryItem` with `created_at`/`updated_at` only, three fused lists (fts, vector, graph), `final_score = fused * (0.5 + importance) * (0.7 + 0.3 * recency)`, five MCP tools.

12 ideas. 9 keep, 3 discard.

---

## 1. Two clocks: when it happened vs when it was learned

**What.** Hindsight stores `occurred_start`/`occurred_end` (the event) separately from `mentioned_at` (the source statement), and resolves every relative date ("yesterday", "last spring") to an absolute date at write time. A fact about 2019 recorded today stays searchable by 2019 and still counts as fresh for recency ranking.
**Verdict.** Keep. v3 has one clock, so `event` memories get a recency boost for being typed in now, which is wrong, and no query can ask "what happened in March".
**Lands in.** `remem-types` (additive `occurred_at: Option<i64>`, `occurred_end: Option<i64>`), `remem-store` (schema columns, filter in `list`/`fts_search`/`knn`), `remem-recall` (`recency_score` reads `occurred_at` when present, falls back to `updated_at`).

## 2. Date window filter, not NL temporal parsing

**What.** Hindsight's fourth arm parses time phrases out of the query, filters to the window, then ranks *by semantic relevance inside the window* and spreads picks across time buckets so "what happened in 2023" does not collapse onto its densest week.
**Verdict.** Keep only the window; discard the parsing. Query-time English date parsing needs an LLM or a date library and a big config surface; the agent already knows the dates and can pass them. Bucket spreading is dead until memory density justifies it.
**Lands in.** `remem-recall` (`since`/`until`/`as_of` on `RecallQuery`), `remem-mcp` and the `remem` CLI (`--since`, `--until`, `--as-of`). `as_of` reuses the same code: rank as if asked at that instant.

## 3. Selectivity rules in the write tool's description

**What.** Hindsight's extraction prompt spends most of its length on what NOT to store (greetings, filler, process chatter, repeats, anything already stated) and ends with one test: "would this be useful to recall in 6 months?" It reports that this single rule removes about 90 percent of output.
**Verdict.** Keep. v3 has no extraction LLM, so the writer is the agent, and the agent reads tool descriptions. Same control point, no engine code, no new model.
**Lands in.** `remem-mcp` (the `remember` tool `description`), mirrored in the `remem` skill doc. Also adopt the tool-description pattern for `recall` (`k` meaning, what a score is).

## 4. Exact-content dedup on write

**What.** LightRAG stores `content_hash` per chunk so a re-ingested chunk is recognised as the same object and merged into the existing graph node instead of creating a second one.
**Verdict.** Keep, in its cheapest form. Agents re-save the same lesson every session; one hash lookup collapses that to zero new rows. While in the write path, wrap insert + embedding + graph projection in one transaction so a mid-write failure cannot leave a row with no vector.
**Lands in.** `remem-store` (`content_hash` column + unique index on `(content_hash, agent_id, session_id)`, `insert` returning the existing id), transaction in `remem-recall::remember`.

## 5. Near-duplicate reconciliation at write time, report-only

**What.** Hindsight's consolidation compares a new or updated observation against its closest existing ones above a cosine threshold (default 0.97) and a focused LLM check answers merge or keep, so the store holds one belief with many proofs instead of ten wordings of the same belief.
**Verdict.** Keep the candidate detection, discard the automatic merge. v3 has a local embedder, so a knn probe costs one forward pass and no API call; deciding whether two memories are the same belief is an LLM judgement, and pulling an LLM into a local-first engine is the wrong trade. Return the neighbours and let the agent call `link` or `forget`.
**Lands in.** `remem-recall::remember` returns `{id, similar: [{id, score}]}` above a threshold; `remem-mcp` surfaces it in the tool result.

## 6. Evidence links and proof counts

**What.** Every Hindsight observation records its supporting fact ids with quotes and carries a `proof_count` (how many facts back it), used as a ranking signal: `proof_norm = clamp(0.5 + ln(count)/10, 0, 1)`, a multiplicative boost capped at +5 percent.
**Verdict.** Keep as a convention, not a feature. v3 already has typed edges, so `remember` a `note`/`decision` and `link(obs, fact, "supports")` gives provenance for free; add the count boost later only if recalled ordering actually shows a problem. No schema change, no new crate.
**Lands in.** `remem-graph` docs note; the read side would be one line in `crates/remem-recall/src/rank.rs` when needed.

## 7. Token-budget result packing instead of top-k

**What.** Hindsight returns results packed to a `max_tokens` budget because agents think in context window, not result counts; a too-long candidate is skipped and packing continues with the next one, and if nothing fits the top hit is returned whole so a matching query never yields an empty list.
**Verdict.** Keep. Two of these are real bugs in v3 today: `--k` can blow the caller's context, and the current `truncate(k)` behaviour is "cut the tail" rather than "skip what does not fit". Cheap, and it is the one interface change callers notice.
**Lands in.** `remem-recall` (packing step after `final_score`), `remem-mcp` + CLI (`--max-tokens`, keep `--k` as the default cap).

## 8. Bounded multiplicative boosts, centered at 1.0

**What.** Hindsight's final score is `ce * (1 + a*(signal - 0.5))` per signal with `a` at most 0.2, so recency, temporal proximity and proof move a ranking by at most about +27/-23 percent. The stated reason for multiplicative over additive: a small bonus must not let an irrelevant-but-recent memory jump a relevant one.
**Verdict.** Keep the shape, fix the constants. v3's `(0.5 + importance)` swings 0.5x to 1.5x (a 3x spread) while recency swings only 0.7x to 1.0x, so a mistyped importance currently dominates fused relevance; importance should be the smaller of the two, not the larger.
**Lands in.** `crates/remem-recall/src/rank.rs` (`final_score`), plus its unit tests.

## 9. MCP tool annotations and argument tolerance

**What.** Hindsight marks read-only tools `readOnlyHint: true`, sets `openWorldHint: false`, and wraps every tool so unexpected extra arguments from an LLM (a stray `explanation` field) are ignored rather than rejected. It also ships a single-bank surface where the scope is implicit and tools take no id parameter.
**Verdict.** Keep the first two, skip the third. `readOnlyHint` lets clients auto-approve `recall`/`list`/`stats` and gate writes; argument tolerance removes a class of silent tool failures that is otherwise invisible in logs. v3 is single-scope by `--db` already, so no surface change is needed.
**Lands in.** `remem-mcp` (`tools_list` annotations; ignore unknown keys in `dispatch`).

---

## 10. Cross-encoder reranking stage -- DISCARD

**What.** After RRF, Hindsight trims to 300 candidates and scores each query-memory *pair* with a cross-encoder, sigmoid-normalising logits to [0,1], then applies its boosts to that score.
**Discard because.** It needs a second resident model plus a batched inference service, doubling memory footprint and startup latency for a local-first CLI. v3's one 768-d BERT does encode-only; if ordering is measurably wrong, fix it with the graph arm and better write-time selectivity (ideas 3 and 5) before adding a model.

## 11. Entity extraction and fuzzy entity resolution to build the graph -- DISCARD

**What.** Hindsight and LightRAG both pull typed entities and binary relations out of every chunk with an LLM, then unify "Alice" / "Alice Chen" / "Alice C." by fuzzy name match reinforced by co-occurrence, and link every fact that shares an entity.
**Discard because.** It is the bulk of both codebases (`fact_extraction.py` 3.5k lines, `entity_resolver.py` 1.5k, `operate.py` 7k) and the entire reason they need an LLM at write time. v3's graph is agent-authored via `link`, which is already the correct entity set, decided by the party that knows the domain. Revisit only if agent-authored edges turn out to be too sparse to expand from.

## 12. Dual-level local/global retrieval with query keyword modes -- DISCARD

**What.** LightRAG extracts `high_level_keywords` (themes) and `low_level_keywords` (entities) from a query, embeds each separately, searches relations/edges with the high-level set and nodes/entities with the low-level set, then round-robins the two lists; `local`/`global`/`hybrid`/`naive`/`mix` pick which arms run.
**Discard because.** The two levels exist because the two vector collections are different (entities vs relationships); v3 has one collection, memories, so both arms would query the same index with LLM-generated paraphrases of it. RRF over fts + vector + graph already covers the consensus behaviour, and mode proliferation is exactly the surface area v3 is trying to avoid.

---

## Sequencing note

Ideas 1, 2, 4 and 8 touch the write path and the score function, so do them together in one schema/ranking pass (schema change is additive; `remem-types` is additive-only, so `Option` fields). Ideas 3, 7 and 9 are text and small control flow, independently shippable. Idea 5 depends on 4 being settled. Idea 6 is a doc line.

Skipped on purpose: async operation queue and webhooks, memory banks/tenant isolation, mental models, disposition traits, reflect's agentic loop, `min_scores` floors, adaptive recall budgets, five pluggable BM25 backends, near-duplicate merge automation.
