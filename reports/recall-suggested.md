# recall-suggested edge producer (issue #15 follow-up)

## Status
- [x] Worktree /home/bindesh/rag/lanes/suggest (branch lane-suggest)
- [x] RED tests (compile-fail RED: 8 missing-method errors): suggestion recorded on graph-fused hit; dedup; prune; list output
- [ ] suggested_edges table (id, from_id, to_id, query, rank, created_at), upsert per pair, 30d prune
- [ ] Engine writes candidate on graph-fused hit (no auto-link)
- [ ] `remem suggestions` CLI lists (id, pair, query, count); accept = `link --rel`
- [ ] cargo test + cargo fmt green, committed

## Design
- Table lives in remem-store schema.sql + migrate() (CREATE TABLE IF NOT EXISTS, so old DBs get it).
- Engine: at fusion, for each hit carrying `graph#` reason, find the seed (from_id) it arrived via; record (from_id, to_id, query, rank) with `record_suggestion`.
- Dedup: unique index on (from_id, to_id), upsert on conflict refreshes query/rank/created_at.
- Prune: DELETE older than 30d, run on record.
- CLI `remem suggestions`: lists rows `id from_id to_id rank count query`.
- NO auto-linking: accepting a suggestion is the existing `remem link --rel`.

## Friction notes
- (dogfooding notes here)
