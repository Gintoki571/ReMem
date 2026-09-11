# remem v3 changelog

## Unreleased / in flight (unmerged working-tree changes)

- Traceable merges (issue #14): corroboration merges record the absorbed phrasing in a `merges` table (`merges_fts` aux); recall searches absorbed content too (appended after direct fts hits, eval 35/37 unchanged); `remem trace` lists absorbed rows under the survivor.
- Graph algos landed: `Graph::central` (PageRank over memory nodes) and `Graph::shortest_path` (directed, empty when unreachable), exposed as CLI + MCP `central`/`path`.
- Score floor landed: `apply_floor` drops recall hits below a minimum score (default 0.0 = off, opt-in `--min-score 0.02`; floor decision tracker #1 closed).
- Abstention guard: content-free recall returns [] with query-empty reason.

## Engine (store/graph/embed/recall/CLI)

- Open-time backfill: pre-existing rows without `content_hash` get hashed on open, so dedup covers old DBs.
- FTS tags column: tag words indexed separately with 2x BM25 weight over content.
- Store `busy_timeout` and limit clamps on read paths.
- Migration runs old-DB column fixes before schema index creation.
- Strict row decoding surfaces corruption instead of silent defaults.
- Importance clamped to 0..1, NaN-safe.
- CLI `forget`/`related`/`central`/`path` subcommands (MCP parity).
- CLI `list --limit` flag for bounded listing.
- Recall `k=0` returns empty instead of defaulting to 5 hits.
- Recall `--json` emits full fields (agent, session, occurred_at, importance).
- Year-bound checked dates on write, glossary terms defined.
- Graph rejects reserved `agent:`/`session:` memory ids (hub collision guard).
- CLI/store path hardening: `~/` without HOME errors, parent-dir failures surfaced.

- Rust memory engine: SQLite store with vector search, graph links, embeddings, and recall CLI.
- `validate` command: checks store/graph consistency and reports issues.
- Recall ranking tuned for recency and importance weighting.
- FTS stopword fix, importance rescale, and second-resolution event clock for ranking.
- Content-hash dedup on write: same kind and content returns the existing id.
- Hard purge in the store: removes the row, FTS entry, and embedding.
- Token-budget packing (`--max-chars`, top hit never dropped) and a `purge` CLI command.

## MCP server

- MCP server over stdio: 11 tools (remember/recall/list/link/forget/purge/stats/validate/related/central/path).
- Read-only annotations and selectivity hints so agents pick sharper tools.
- `validate` and `forget` tools (empty validate result means healthy).
- `purge` tool: hard-deletes a forgotten memory.
- `related` tool: graph neighbors of a memory id, with optional edge-type filter.
- `recall` accepts `maxChars`, passed through to token-budget packing.
- `recall` accepts `minScore` floor passthrough and `since`/`until` time filters.
- `central`/`path` MCP tools with `get()` caller fix.
- `remember` accepts `occurredAt`, matching the CLI `--occurred-at` flag.
- `remember` matches the committed `String` engine API; MCP coercion errors surfaced.
- Near-duplicate report on write: `similar` array (`SIMILAR_MAX_DISTANCE` 0.48), floor stays opt-in.
- MCP hardening: 16 MB frame cap with JSON-RPC error replies on malformed frames, top-k clamped to 1000, DB dirs created 0700 and DB files kept 0600.

## Quality (eval/validate/fixes)

- Tag boost 1.05x with narrowed multiplier bands (post-tag-boost eval 62% @1, 100% @5).
- Learned ranking weights: RRF k 60 to 30, vector list half-weighted (0.5), importance dropped from the final score, usable floor band measured at 0.017..0.043 (floor stays off by default).
- Gated tag boost: 2.0x only for FTS-absent hits with tag/query overlap, 35/37 @1.
- Floor-first ordering: `apply_floor` runs on unboosted scores before the gated tag lift, so junk cannot be multiplied past the floor (verifier GO 35/37 @1, 35/37 @5 floored, adversarial 3/3; Q27 floor-dropped by design).

- Recall eval harness: 16% @1 / 64% @5 on the probe set, with notes.
- Release performance numbers published.
- Eval fixtures grown from 25 to 40.
- 40-fixture eval: fused recall@1 31/37 (84%), recall@5 37/37 unfloored (35/37 floored at `--min-score 0.02`); single signals 33/37 FTS-alone, 34/37 vector-alone.
- Release parity check: binary sizes, MCP tool count, and a remember/recall roundtrip.
- Adversarial battery 6/10 post-guard (up from 4/10; #4/#9 flipped via abstain-empty).
- `cargo test --workspace` (HEAD `2350334`): 153 passed, 0 failed, 2 ignored.
- Trackers #1-#4 closed; #5/#6 open.

## Docs

- README-v3: Rust workspace guide, CUDA notes, and agent skill.
- AGENTS.md contract and remem agent skill.
- Prime-agent MCP registration docs.
- Provider fallbacks guide.
- CLI flags and MCP tools refresh.
- Help text for every CLI flag (`remem --help` covers kind/tags/agent/session/importance/query/k/json/filters).
- Demo covers graph commands (`link`/`related`/`central`/`path`) in `docs/demo.md` and `scripts/demo.sh`.
- Security review notes.

## Docs (catch-up)

- Landing checklist, MCP `similar[]` contract, skill similar/floor notes, README quality pass.
- Parity map refresh, roadmap 31/37 confirmed, release checks 2 and 3 green, goal audit (7/7 met).
- Stale-claim sweep across 10 files; dogfood digest; merge-to-main recon.

## CI

- Rust toolchain via dtolnay action for reliable CI builds.
- Demo e2e CI job without model; demo script respects `REMEM_DB`.
- Full-workspace `fmt` pass and clippy `err_expect` fix in MCP test.
- Unified duplicate deps: tokenizers 0.22, zerocopy 0.8 (dropped 0.23/0.7 splits).
