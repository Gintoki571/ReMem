# recall-suggested edge producer (issue #15 follow-up)

## Status
- [x] Worktree /home/bindesh/rag/lanes/suggest (branch lane-suggest)
- [x] RED tests: suggestion recorded on graph-fused hit; dedup; prune; list output (compile-fail RED, commit 75ebea6)
- [x] suggested_edges table (id, from_id, to_id, query, rank, count, created_at), upsert per pair, 30d prune
- [x] Engine writes candidate on graph-fused hit (no auto-link)
- [x] `remem suggestions` CLI lists (id, pair, query, rank, count); accept = `link --rel`
- [x] cargo test + cargo fmt green, committed (87a6167)

## Design
- Table in remem-store schema.sql (`CREATE TABLE IF NOT EXISTS`, so old DBs get it on open).
- Unique index on (from_id, to_id); `record_suggestion` upserts (refresh query/rank/created_at, bump count). Pair is canonicalized lexicographically because the graph walks edges in both directions and the same pair arrives both ways.
- `prune_suggestions` (30-day window) runs on every record.
- Engine: during graph expansion, `edge_from` maps each fused-in id to the seed that pulled it; after final ranking, every hit with a `graph#` reason records `(seed -> hit, query, rank)` — 1-based position in the final returned list. Recording failures are non-fatal (suggestion only).
- CLI `remem suggestions`: `id  from -> to  rank N  count N  query`; empty -> "no suggestions". Accepting a candidate stays the existing manual `remem link --rel`. NO auto-linking anywhere (regression guard test included).

## Tests (crates/remem-recall/tests/suggestions.rs, RED-first)
- suggestion_recorded_on_graph_fused_hit
- fts_only_recall_records_no_suggestion
- suggestions_dedup_upserts_per_pair (one row per pair; count accumulates — across two recalls both directions fuse, so count 3 is correct, not 2)
- suggestions_pruned_after_30_days (via age_suggestions_for_test backdate hook)
- suggestions_do_not_auto_link

## Dogfood friction notes
- `embedder: Cpu (768d)` banner prints on every command, including read-only ones (suggestions, list). It is on stderr, so pipes are safe, but it makes `| grep` debugging noisy. Consider printing only when an embed op actually runs.
- `remem suggestions` has no `--json` flag (recall has one); text-only output is harder to consume programmatically.
- Empty result prints "no suggestions" to stdout — fine for humans; a --json mode would prefer `[]`.
