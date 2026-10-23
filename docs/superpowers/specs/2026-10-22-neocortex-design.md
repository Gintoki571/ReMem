# neocortex 0.1.4 — design

Status: draft for approval. Clean rewrite of ReMem (pre-rewrite snapshot: tag `remem-final`, pushed).

## 1. Intent

neocortex is a long-term memory that AI agents (Claude Code, Copilot, opencode, …) write to and read from while they work. It is general purpose (people, places, preferences, projects, events, decisions, lessons), not a code-retrieval tool. It is built from the agent's point of view:

- Writing must be cheap and forgiving. A bare sentence is enough. An agent that already knows the structure (entities, relations, time) can hand it over as JSON, and neocortex uses it instead of guessing.
- Reading must be one call that returns the right few things, compact, inside a token budget, with enough provenance (ids, dates, validity) to act on.
- Memory must stay correct over time. Facts change ("moved from Delhi to Pune"). Old facts are kept as history, not deleted, and are not presented as current truth.
- Different words for the same or related things must still connect (house/home, mother/parent/guardian, bros/friends, Postgres/PostgreSQL).

## 2. Goals, non-goals, anti-goals

Goals
- Four retrieval channels: BM25 keyword (FTS5), dense vector (sqlite-vec), graph (personalized PageRank over an entity/memory graph in graphqlite), temporal (time-window and validity aware), fused with weighted RRF.
- A bi-temporal fact model: when something was true in the world vs when the system learned or retired it, with as-of queries.
- Entity and relation resolution that merges only on strong evidence and links near-synonyms softly.
- UltraBERT-Embed-150M embeddings implemented in pure Rust on candle, numerically matching the reference implementation.
- One self-contained binary `neocortex` (CLI + MCP stdio server), version `0.1.4`, one SQLite file per user.
- Measurable quality: an eval suite with per-channel ablation; dogfooded by the agents building it.

Non-goals (0.1.4)
- No LLM inside neocortex. No generative extraction, summarisation or answer synthesis; the calling agent is the LLM.
- No GLiNER NER yet: GLiNER2.5-small is a custom "boundary" architecture, and porting it is a separate project. Recorded as a future option.
- No server or network mode, no multi-user auth, no cloud sync.
- No GPU requirement: CPU is the default and the supported path, and a candle CUDA feature is optional/best-effort.

Anti-goals (would be a failure even if it "works")
- Silent wrong merges, e.g. mother merged into guardian, or London into Paris.
- Returning superseded facts as if they were current.
- A sprawling tool surface (dozens of overlapping MCP tools) or verbose output that eats the agent's context.
- Requiring Python, ONNX runtime, Docker, or any running service.

## 3. Constraints

- Rust stable, edition 2021, single crate `neocortex` (lib + bin). Version `0.1.4`. Target: Linux x86_64 (tested); macOS/Windows best effort (graphqlite bundles their extension too).
- SQLite via `rusqlite` (bundled). Extensions: `sqlite-vec = "=0.1.9"` (0.1.10-alpha does not compile) and `graphqlite = "0.12"` (bundled loadable extension, extracted at runtime by the crate). One connection per process, used for everything, so writes are a single transaction: `Engine` owns a `graphqlite::Connection` built with `Connection::from_rusqlite` (sqlite-vec registered first via `sqlite3_auto_extension`) and runs relational SQL through `sqlite_connection()`. Cypher projection writes happen inside the same transaction; if graphqlite cannot run inside an open transaction, the builder must report that before choosing a fallback.
- Embedding model dir `UltraBERT-Embed-150M` (safetensors f32, 24 layers, hidden 768, 6 heads × head_dim 128, RoPE with per-layer theta 10k or 2.56M, sliding-window attention ±128 on 2 of every 3 layers, RMSNorm eps 1e-6, SwiGLU MLP 1024, final norm, CLS pooling at token 0, L2 normalise). Max input: 1024 tokens (`NEOCORTEX_MAX_TOKENS`); longer content is truncated for the embedding only, while stored text and BM25 see the whole content. `remember` returns a hint when content is over 2000 chars, because atomic memories retrieve better. Tokenizer: byte-level BPE `tokenizer.json`, which prepends `<|endoftext|>` (id 0). Query prompt: `"Instruct: Given a question, retrieve passages that answer the question\nQuery: "`; documents have no prompt. Model dir from `NEOCORTEX_MODEL_DIR`; default `~/.neocortex/models/UltraBERT-Embed-150M`.
- Allowed crates: rusqlite, sqlite-vec, graphqlite, candle-core, candle-nn, tokenizers (only the features needed for byte-level BPE), serde, serde_json. Anything else needs a written reason in `AGENTS.md`. Hand-roll arg parsing, ids, hashing (FNV-1a 64 + content compare), time math and JSON-RPC.
- DB path: `--db` or `NEOCORTEX_DB`, default `~/.neocortex/neocortex.db`. Dirs 0700, files 0600.

## 4. Architecture

```
src/
  main.rs       CLI dispatch (hand-rolled args) + `mcp` subcommand
  lib.rs        pub use; Engine
  db.rs         open, extensions, schema.sql, schema_meta version ladder
  schema.sql
  embed.rs      Embedder trait; UltraBERT (candle); HashEmbedder for tests
  ultrabert.rs  model forward pass
  text.rs       normalisation, singularisation, stopwords, Jaro-Winkler, trigram Jaccard
  temporal.rs   date/time parsing (absolute + relative), intervals, bi-temporal predicates
  extract.rs    heuristic entity extraction + alias gazetteer matching
  resolve.rs    entity and predicate resolution
  graph.rs      graphqlite projection, adjacency load, ACL push PPR, Louvain communities
  remember.rs   write pipeline
  recall.rs     channels, fusion, context assembly, budget packing
  mcp.rs        JSON-RPC 2.0 stdio MCP server
tests/          integration tests (HashEmbedder; real model tests skip when model dir absent)
eval/           fixtures + `neocortex eval` runner
```

`Engine` owns the connection and an `Arc<dyn Embedder>`. The MCP server is long-lived: the model is loaded once and the graph adjacency is cached and invalidated by a write generation counter. The CLI pays model load per call (mmap safetensors).

## 5. Data model (SQLite)

Ids are integers, shown to agents with a prefix (`m12`, `e7`, `r33`). Short ids save tokens, and any tool accepts the id with or without its prefix.

- `memories(id, content, kind, tags, importance REAL 0..1, scope, source, session, created_at, valid_from, valid_to NULL, expired_at NULL, superseded_by NULL, access_count, last_access, content_hash)`
  - `kind` ∈ {fact, event, decision, preference, mistake, procedure, observation, note}; any other value is rejected with the list.
  - One live row per `content_hash` (unique partial index `WHERE expired_at IS NULL`); a hash hit is a duplicate only when kind and folded content also match. The schema version lives in `schema_meta`, not `PRAGMA user_version` (graphqlite owns that pragma).
  - World time: `valid_from` (defaults to created_at) and `valid_to` (NULL = still true or unknown). System time: `created_at` and `expired_at` (set by forget/supersede).
- `memories_fts` FTS5 (content, tags), porter unicode61, external content + triggers.
- `memories_vec` vec0 `embedding float[768] distance_metric=cosine`, plus a metadata column `live` (so KNN skips expired rows without oversampling); rowid = memory id.
- `entities(id, name, norm UNIQUE, type, description, created_at, updated_at, mention_count)`; `entity_aliases(alias_norm, entity_id, source: agent|lexical|manual)`; `entities_fts(name, aliases, description)`; `entities_vec` (embedding of `"name (type): description"`).
- `mentions(memory_id, entity_id)` — the passage↔phrase bipartite edges (HippoRAG).
- `predicates(id, canonical)`; `predicate_aliases(alias_norm, predicate_id)`.
- `relations(id, src, dst, predicate_id, fact, weight, evidence_count, valid_from, valid_to, created_at, expired_at)`; `relation_evidence(relation_id, memory_id)`; `relations_fts(fact, predicate)`; `relations_vec` (embedding of the fact sentence; when the agent gives no sentence, `"src predicate dst"`).
- `similar(a, b, score)`: soft near-synonym links between entities (a < b).
- `communities(entity_id, community, computed_at)` (Louvain, recomputed lazily).
- graphqlite projection in the same file: nodes `e:<id>` (label Entity) and `m:<id>` (label Memory); edges `MENTIONS` (m→e), `SIMILAR` (e–e, score), and one edge per relation typed by sanitised predicate with `rid`. The projection is derived from the relational tables, which are the source of truth for attributes and time. `neocortex reindex` rebuilds it. `validate` reports drift.

## 6. Write path — `remember`

Input (CLI flags or MCP JSON):
```json
{"content": "Mom moved from Delhi to Pune last month.",
 "kind": "event", "tags": ["family"], "importance": 0.7, "scope": "personal",
 "valid_from": "last month",
 "entities": [{"name": "Mom", "type": "person", "aliases": ["mother"], "description": "the user's mother"},
              {"name": "Pune", "type": "place"}],
 "relations": [{"source": "Mom", "predicate": "lives in", "target": "Pune", "exclusive": true,
                "fact": "Mom lives in Pune"}],
 "keywords": ["relocation"], "supersedes": ["m12"]}
```
Steps, in one `BEGIN IMMEDIATE` transaction (embeddings computed before the transaction opens):
1. Validate input. Parse `valid_from`/`valid_to` (ISO date/datetime, unix seconds, or relative phrases, anchored at now).
2. Exact duplicate (same content hash + kind + folded content, live): bump access/evidence, return the existing id with `duplicate_of`. The unique index enforces it under concurrency.
3. Embed content (document mode). If the top live KNN neighbour has cosine ≥ `NEAR_DUP` (calibrated; starting value 0.97), report `near_duplicate: mN` and still store the new memory. Never silently drop.
4. Entities = agent-supplied entities ∪ gazetteer matches (known aliases found as whole-word n-grams in the content) ∪ heuristic candidates. Heuristic candidates are used only when the agent supplied no entities: proper-noun spans, quoted spans, @handles, #tags, URLs, paths, identifiers. Resolve each one (§7), then insert `mentions`.
5. Relations: resolve endpoints (unknown names create entities), canonicalise the predicate (§7), then upsert:
   - same (src, predicate, dst) live: weight/evidence += 1, add evidence, keep the earliest valid_from;
   - `exclusive: true` (or the predicate is marked exclusive by an earlier exclusive write): every other live relation with the same (src, predicate) and a different dst gets `valid_to = new.valid_from` and `expired_at = now`, and is reported as `invalidated`.
6. `supersedes`: each target memory gets `valid_to` (if unset) = new valid_from, `expired_at = now`, `superseded_by = new id`, and its vec `live = 0`. Relations the target alone still evidenced get `valid_to = new valid_from` and keep `expired_at` NULL (true until then). `forget` does the same with `now`, also setting `expired_at`; hard forget deletes relations it alone evidenced and clears `superseded_by` pointers at it.
7. Update the graphqlite projection and bump the write generation.
8. Return a compact result: id, how each entity resolved (`new` | `matched e7` | `alias-of e7` | `similar-to e9 (0.81)`), relations created/merged, invalidated ids, and duplicate notes.

### Temporal rules
- Validity intervals are half-open `[valid_from, valid_to)`; NULL `valid_to` = open. "Valid at T" ⇔ `valid_from <= T AND (valid_to IS NULL OR T < valid_to)`. Live (system time) ⇔ `expired_at IS NULL`. An as-of query uses world time only; system time decides what is "current knowledge".
- Relations carry polarity (`1` positive, `0` negative) and `ended_by`. A matching negative assertion ends a live positive interval at its world start and remains as its own evidence; explicit `invalidate` leaves `ended_by` NULL. Reasserting a past interval attaches evidence to the existing same-triple, same-polarity row when intervals overlap or touch.
- `exclusive` is tri-state: `true` ends competing values, `false` never ends or reports them for that write, and omitted applies a seeded predicate policy or returns bounded conflict candidates. Non-exclusive predicates remain multi-valued by default; history is never deleted.
- Text-only update candidates use `Config.update_candidate = 0.82`. A 10-pair UltraBERT-Embed-150M calibration measured update cosine min/max/mean `0.6974/0.9724/0.8802` versus related-but-not-update `0.3335/0.6588/0.4697`; this threshold favors high precision while returning hints only.
- Exclusive invalidation compares world time:
  - an older live relation (`old.valid_from < new.valid_from`) gets `valid_to = new.valid_from`;
  - a backfilled fact (`new.valid_from < old.valid_from`) is stored already ended: `new.valid_to = min(existing later valid_from)`, and it is history;
  - equal valid_from: the newer write wins, and the old one gets `valid_to = valid_from` and `expired_at`.
  Invalidated relations keep their rows; `expired_at` is set only when the new write makes the old fact wrong (equal start). An ended-but-true-in-the-past fact keeps `expired_at` NULL with a non-NULL `valid_to`.
- A memory whose evidenced relations have ALL ended gets `valid_to` = the latest relation end (if still open), including an equal start (empty interval, shown `[ended]`): "Alex lives in Austin" becomes history once "lives in Denver" is written. A memory that still evidences a live fact stays open.
- The reverse holds too: a memory with an end (`--until`, `invalidate m<id>`, or as the superseded one) ends the relations only it keeps current, at that time, never later than their current end. `validate` reports a live relation whose every evidence memory has ended.
- `invalidate` keeps the earliest end: re-invalidating answers `already ended at <date>` and changes nothing.
- `forget` (soft) retracts the relations only it evidenced: `expired_at = now`, `valid_to` untouched (no invented end date). Retracted facts render as `retracted`, not `ended`.
- `--when "from X to Y"` / `"between X and Y"` sets `valid_from` and `valid_to` (exclusive end). `N weeks ago` / `N months ago` store that exact day; `last <month>` is the bare month, as in recall.
- Without `--when`, the one explicit date in the content (`March 5, 2025`, `in 2019`, `2027-06-15`, `yesterday`, `3 days ago`) becomes `valid_from`, reported as `when: inferred from text '...'`. Several dates, or relative phrases (`next month`), infer nothing. No recurrence is invented.
- Exclusivity is a property of the canonical predicate. It is global (entities are global across scopes) and applies per source entity: `(src, predicate)` has at most one valid object at any world time.

## 7. Entity and relation resolution

Calibration on UltraBERT (bare names, document mode) showed embeddings alone cannot tell "same" from "related": same pairs scored 0.63–0.93 (house~home 0.775, bros~friends 0.70), related-but-different pairs 0.48–0.81 (London~Paris 0.807, mother~parent 0.808). So:

Entity resolution for a mention `(name, type?, description?, aliases?)`:
1. `norm(name)`: Unicode NFKC-lite fold (lowercase plus common accent stripping), trim punctuation, collapse whitespace, drop leading articles/possessives (`the`, `a`, `my`, `'s`), and singularise the last token with English rules (houses→house, bros→bro, but not `analysis`, `status`, `bus`).
2. Exact: `norm` or any agent-supplied alias equals an existing alias. If the alias names exactly one entity whose type is compatible (equal, or one side unknown), match it. If it names several, pick by embedding cosine with the mention's description/context.
3. Lexical merge (strong evidence only), against candidates from alias trigram blocking plus the top-10 entity KNN:
   - Jaro-Winkler(norm) ≥ 0.93 and both norms ≥ 5 chars, with no differing digits and no differing content token (guards: `v1`/`v2`, `get_user`/`set_user`, `Paris TX`/`Paris`) → merge as alias (Postgres/PostgreSQL);
   - acronym match: initials of a multi-token name equal the other name (NYC/New York City) and embedding cosine ≥ 0.6 → merge as alias.
4. Agent override: `aliases` / `same_as` in the payload, and the `merge_entities` tool, always merge.
5. Otherwise create a new entity. If entity cosine ≥ `SIM_EDGE` (starting value 0.75, calibrated on the eval set), add a soft `similar(a, b, cos)` edge to at most the top 3. These edges carry graph activation (home→house, mother→parent) but never merge nodes.
6. Never merge transitively: a mention merges into one existing canonical entity, and that entity's aliases do not chain-merge other entities.

Predicate resolution: `norm_pred` = lowercase, words split on non-alphanumerics, auxiliaries dropped (is/are/was/has/have/been/currently), and each verb lemmatised by suffix rules (works/working/worked→work). The predicate canonical key is the joined lemma string (`lives in`, `live_in`, `lived in` → `live in`). The exact alias table goes first. A new predicate whose embedding cosine with an existing predicate is ≥ 0.90 becomes an alias of it. Exclusive-ness is a property of the canonical predicate.

## 8. Recall

Input: `query`, `k` (default 8), `budget` tokens (default 1200, approx chars/4), filters (`kinds`, `tags`, `scope`, `source`, `session`, `since`, `until`), `as_of` (world-time snapshot), `include_history` (default false), optional agent-supplied `entities` / `keywords` (LightRAG low/high-level keys).

1. Temporal analysis: parse time expressions in the query (today, yesterday, last/this/next week|month|year, N days/weeks/months ago, weekday names, month names with or without a year, years, ISO dates, since/before/after/between, recently). The result is a soft window W. Explicit `since`/`until` are hard filters on interval overlap (`[valid_from, valid_to)` meets `[since, until)`) for memories and facts alike. A query window that overlaps no live memory is reported (`window_empty`) and does not filter results. History intent (used to, previously, before, formerly, history, changed, was … until, `include_history`) turns off the expired penalty.
2. Channels (each is a ranked list of memory ids, depth `N = max(50, 5k)`):
   - **bm25**: FTS5 `MATCH` over the stopword-filtered OR of query terms (and keywords), with `bm25(memories_fts, 1.0, 2.0)`.
   - **vector**: query-mode embedding, KNN on `memories_vec` over live rows (plus expired rows when history or as_of applies).
   - **graph** (HippoRAG 2 style):
     - Seeds:
       - (a) alias n-gram exact matches in the query and in agent-supplied entities, weight 1.0;
       - (b) entity KNN (query embedding vs `entities_vec`) with cosine ≥ 0.55 (calibrated), weight = cosine;
       - (c) relation-fact hits (`relations_fts` + `relations_vec`, top 10), whose endpoints get 0.5 × score;
       - each seed weight × specificity `1/|mentions(e)|` (floor 1), normalised to mass 0.95;
       - memory seeds from the vector channel's top 20 (cosine scores), normalised to mass 0.05.
     - Graph: undirected weighted; memory–entity 1.0, relation edges `weight_norm = ln(1+evidence)`, similar edges = score. Only relations valid at `as_of` (default: now, plus expired ones when history applies).
     - Walk: Andersen–Chung–Lang (ACL) local push PPR — implemented in neocortex, because graphqlite only has global PageRank — teleport α = 0.5, ε = 1e-5 (degree-normalised), with work capped at 200k pushes.
     - Output: memory nodes ranked by PPR mass.
   - **temporal** (only when W exists): memories whose `[valid_from, valid_to)` overlaps W, scored by cosine to the query (brute force over the window when it holds ≤ 5000 rows, otherwise KNN with oversampling and a filter).
3. Fusion (calibrated 2026-10-23 on eval/fixtures.json and validated on the independent eval/holdout.json with `neocortex eval --sweep`; defaults in `engine::Config`: rrf_k 5, w_vector 2, w_graph 1.5, w_bm25 1, w_temporal 1, memory-seed share 0.6, history boost 1.5, MMR lambda 0.9 — a low RRF k lets a strong single-channel hit such as a PPR-only multi-hop answer win; the original values below were the starting point): weighted RRF, `score = Σ_c w_c / (60 + rank_c)`, defaults `w = {bm25: 1.0, vector: 1.0, graph: 1.0, temporal: 1.0}` (tuned by eval), then × (1 + 0.05 × (channels_agreeing − 1)).
4. Modifiers (multiplicative, small, so relevance dominates):
   - expired or invalidated without history intent: × 0.35;
   - ended, with history intent and no as_of: × history_boost (1.5) — the past state is the answer;
   - inside W: × 1.25;
   - recency: × (0.9 + 0.1 · 2^(−age_days/90)) on valid_from;
   - importance: × (0.9 + 0.2 · importance).
5. Diversity: MMR (λ = 0.75) over the top 3k candidates using stored embeddings, which removes near-duplicate hits.
6. Context assembly (graph RAG output):
   - the top memories;
   - up to 8 facts: relations among the top-PPR entities, valid at as_of (or with validity shown), ranked by PPR(src)+PPR(dst) and fact relevance;
   - up to 3 entity cards (name, type, aliases, description).
   - Everything is packed into the budget: whole items only; the top memory always fits (`over_budget` is set when it alone exceeds the budget); omissions are reported.
7. Side effect: bump `access_count`/`last_access` of returned memories (recorded only; not used in ranking unless eval shows benefit).

Default output (text, compact; `json: true` gives the same structure as JSON):
```
memories (5 of 23, ~410/1200 tokens):
[m42] event 2026-09-14 · Mom moved from Delhi to Pune.  (bm25#2 vec#1 graph#1)
[m12] fact 2025-03-02→2026-09-14 [ended] · Mom lives in Delhi.  (graph#2)
facts:
[r9] Mom —live in→ Pune (since 2026-09-14)
[r4] Mom —live in→ Delhi (2025-03-02 → 2026-09-14, ended)
entities: [e3] Mom (person; aka mother) · [e8] Pune (place)
```

## 9. Agent interface

MCP (stdio, JSON-RPC 2.0, newline-delimited; protocol version negotiated) tools, kept small and typed:

| tool | purpose |
|---|---|
| `remember` | store a memory, optionally with entities/relations/time (schema §6) |
| `recall` | retrieve (§8) |
| `entity` | profile of one entity: aliases, description, current facts, past facts, similar entities, and a memory timeline (paged) |
| `invalidate` | mark a memory or relation as no longer true from a time (`valid_to`), keeping it as history |
| `forget` | retire a memory (soft, sets expired_at), or `hard: true` to delete it with its vectors/mentions/evidence |
| `merge_entities` | fix resolution: merge b into a (aliases, mentions, relations move), or `alias` to add a name |
| `overview` | global view: top communities relevant to a query or overall, with key entities and facts |
| `stats` | counts, db size, model status |

MCP: protocol versions `2025-06-18`, `2025-03-26`, `2024-11-05` (echo the client's if supported, else the newest); capabilities `tools` only; tool errors return `isError: true` content, and protocol errors return JSON-RPC errors. Targets: Claude Code, Copilot CLI, opencode, qoder, codebuddy, agy.

CLI mirrors every tool (`neocortex remember …`, `recall`, `entity`, `invalidate`, `forget`, `merge`, `overview`, `stats`) plus `mcp`, `reindex`, `validate`, `eval <fixtures>`, `bench`, `cypher <read-only query>`, `import-remem <remem.db>` (one-shot import of a ReMem v3 DB: live memories with kind/tags/time are re-embedded with UltraBERT; ReMem graph edges are not imported). The DB format is new; no ReMem schema compatibility. Every command supports `--json`.

Agent skill doc `skills/neocortex/SKILL.md`: when to remember (decisions, preferences, mistakes and their fixes, facts about people/projects/environment, outcomes), how to pass entities/relations, when to recall (start of a task, before decisions, when the user references the past), and how to invalidate instead of contradicting.

## 10. Verification

- Unit tests per module (text, temporal parser, resolution, PPR on toy graphs against power-iteration PPR, RRF, packing, MCP framing).
- Embedding parity (the fixture is authoritative; it was produced with sentence-transformers 6.2.0, transformers 5.20.2, torch 2.14.1 CPU fp32, `trust_remote_code`, eager attention; docs encoded with batch_size 2 and right padding): `tests/fixtures/ultrabert_fixture.json` holds reference ids + embeddings from the official Python implementation for 5 docs (including a 752-token text that exercises the sliding window) and 3 prompted queries. Token ids must match exactly, and cosine(rust, ref) must be ≥ 0.999 for every vector.
- Eval suite `eval/fixtures.json`: ≥ 80 memories with entities/relations/time and ≥ 60 queries in categories keyword, paraphrase, synonym-entity, multi-hop, temporal-window, as-of, superseded-fact, preference, and adversarial-no-answer. `neocortex eval` prints recall@1/@5, MRR per category, plus an ablation per channel. Targets: overall recall@5 ≥ 0.90; every channel's ablation either costs recall or is justified; superseded facts never ranked above current ones without history intent.
- Performance (12-core CPU, warm MCP server, 10k memories): recall p50 ≤ 150 ms, remember p50 ≤ 200 ms; cold CLI recall ≤ 2 s. `neocortex bench` reports these.
- Gates: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.

## 11. Decisions I made (please check)

- Agent-supplied JSON is the primary extraction path and the heuristic extractor is the fallback (user approved). GLiNER is deferred.
- Clean rewrite as one crate in the same git history (repo renamed ReMem -> Gintoki571/neocortex, `main`); old code at tag `remem-final`.
- Embedding-only similarity never merges entities; it only creates soft `similar` edges.
- Relational tables are the source of truth; the graphqlite graph is a rebuildable projection used for Cypher (`cypher` command), paths and Louvain communities. The PPR adjacency snapshot is read from the relational tables (mentions, relations, similar), so as-of validity filtering is plain SQL and does not depend on graphqlite's internal table layout; it is cached per write generation.
- Integer ids with prefixes.
- Closed `kind` set.
- No automatic deletion or decay of memories; time affects ranking only.
- Times are stored as UTC unix seconds. Relative phrases and displayed dates use `NEOCORTEX_TZ` (±HH:MM) or, by default, the system's local UTC offset (read through SQLite's localtime, sampled once per process — a DST switch is seen after restart).
- Thresholds (NEAR_DUP 0.97, SIM_EDGE 0.75, entity seed 0.55, predicate alias 0.90, RRF weights) are starting values, and the eval suite tunes them.

## 12. Left to the builder

Exact SQL and indices, module-internal APIs, the heuristic extractor's rules, the text output layout details, batching/caching strategy in the embedder, and the community refresh trigger.
