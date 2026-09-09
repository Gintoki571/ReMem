# Migration Gap: content_hash Not Backfilled

## Scenario
Database already has `occurred_at` column but lacks `content_hash` column.

## Code Path (lib.rs)
1. `open_path()` lines 57-68: ALTER TABLE loop adds `content_hash TEXT` (catches "duplicate column" error if already present)
2. `open_path()` line 70: `conn.execute_batch(SCHEMA)?` — schema.sql creates `idx_memories_content_hash` with `IF NOT EXISTS`
3. `migrate()` lines 172-190: `has_col("content_hash")` returns true (added in step 1), so ALTER TABLE is skipped; unique index created with `IF NOT EXISTS`

## The Hole
**No backfill UPDATE** populates `content_hash` for existing rows. They remain `NULL`.

## Consequences
- `find_by_hash()` (line 87) only matches non-NULL hashes — old rows invisible to deduplication
- Unique index `idx_memories_content_hash` permits multiple NULLs (SQLite semantics)
- New inserts get correct `content_hash`; old rows never do unless manually updated

## Evidence
- lib.rs:57-68 — ALTER TABLE loop, no backfill
- lib.rs:172-190 — migrate() checks column existence only, no UPDATE
- schema.sql:12 — `content_hash TEXT` (no DEFAULT, no NOT NULL)
