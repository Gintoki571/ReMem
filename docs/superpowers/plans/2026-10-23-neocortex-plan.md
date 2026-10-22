# neocortex 0.1.4 — build plan

Design: `../specs/2026-10-22-neocortex-design.md`. Orchestrator merges every lane after review + gates (fmt, clippy -D warnings, test).

## Phase 0 — scaffold (orchestrator) ✅
Cargo.toml, schema.sql, db.rs (verified: sqlite-vec + graphqlite on one connection, vec0 `live` metadata filter, Cypher rollback), module stubs = contracts, UltraBERT reference fixture.

## Phase 1 — independent modules (parallel lanes, TDD)
| lane | harness | files | done when |
|---|---|---|---|
| embed | copilot | embed.rs, ultrabert.rs, tests/embed_parity.rs, examples/bench_embed.rs | token ids exact + cosine >= 0.999 on all fixture vectors; HashEmbedder tests |
| temporal | codebuddy | temporal.rs | parser table tests incl. relative phrases, months, years, ranges, tz offset, history intent |
| text | qoder | text.rs, extract.rs | unit tests for every fn incl. guards (analysis/status, Paris TX, v1/v2) |
| graph | agy | graph.rs, rank.rs | ppr_push within 1e-3 L1 of ppr_power on random graphs; rrf/mmr/pack tests |
| eval | opencode | eval/fixtures.json, eval/README.md | >= 80 memories, >= 60 queries, all categories |

## Phase 2 — engine (sequential, after Phase 1 merge)
1. kg.rs: graphqlite projection writes + adjacency snapshot (generation-cached) + Louvain communities.
2. resolve.rs: entity + predicate resolution (design §7).
3. remember.rs: write pipeline (design §6) incl. temporal rules.
4. recall.rs: four channels, fusion, modifiers, MMR, context assembly, packing (design §8).
5. engine.rs facade; entity/invalidate/forget/merge/overview/stats.

## Phase 3 — interfaces
mcp.rs (stdio JSON-RPC, 8 tools), main.rs CLI (all commands, --json), import-remem, skills/neocortex/SKILL.md, README.

## Phase 4 — quality loop
`neocortex eval` + `bench`; calibrate thresholds/weights; channel ablations; dogfood: register MCP server, import ReMem memories, use neocortex while developing; iterate on failures.
