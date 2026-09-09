# Ops guide (v3 store)

Verified 2026-09-08 against a scratch DB built from
`crates/remem-store/schema.sql`, using the system `sqlite3` CLI (3.53.4)
for file ops and python3/stdlib `sqlite3` for inserts.
The CLI build has no `vec0` module, so `mem_vec` could not be
created in the scratch DB; everything below covers the relational +
FTS side. `Store::open` sets `journal_mode=WAL`.

## File layout

One DB file plus WAL sidecars while running: `<name>.db`, `<name>.db-wal`,
`<name>.db-shm`. Sidecars disappear after a clean close + checkpoint.

Tables in a fresh file (`.tables`):

```sh
sqlite3 memories.db ".tables"
# memories  memories_fts  memories_fts_config  memories_fts_data
# memories_fts_docsize  memories_fts_idx
# (+ mem_vec in the app, via the sqlite-vec extension)
```

```sh
sqlite3 memories.db "SELECT name, type FROM sqlite_master;"
# memories|table
# idx_memories_content_hash|index
# memories_fts|table (+ _data, _idx, _docsize, _config)
# memories_ai|trigger, memories_ad|trigger, memories_au|trigger
# mem_vec|table (app only, vec0 embedding FLOAT[768])
```

## Backup / restore

Use `.backup` (online-safe, copies WAL state) or `VACUUM INTO`
(compacts at the same time). Both verified: backup opens clean,
`integrity_check` returns `ok`, row count matches.

```sh
sqlite3 memories.db ".backup memories-backup.db"
sqlite3 memories.db "VACUUM INTO 'memories-vacuum.db';"
sqlite3 memories-backup.db "PRAGMA integrity_check; SELECT count(*) FROM memories;"
```

Restore is a plain file copy while all writers are stopped:

```sh
# stop every writer first, then
cp memories-backup.db memories.db
sqlite3 memories.db "PRAGMA integrity_check;"
```

Back up (or copy) the single `.db` file only; never copy `-wal`/`-shm`
without the main file.

## Integrity

```sh
sqlite3 memories.db "PRAGMA integrity_check;"
# ok
sqlite3 memories.db "PRAGMA journal_mode;"   # expect: wal
sqlite3 memories.db "PRAGMA page_size; PRAGMA page_count; PRAGMA freelist_count;"
```

If `integrity_check` reports anything other than `ok`, restore from the
last good backup; do not keep writing to the file.

## Size math

Measured: empty file 32768 bytes (8 x 4096 pages). After inserting 100
memories with ~190-char content (+ FTS index rows), 77824 bytes.
Delta 45056 bytes = ~450 bytes per memory at that content size.

```sh
sqlite3 sized.db "SELECT count(*), avg(length(content)) FROM memories;"
# 100|189.9
```

Rule of thumb: ~0.5 KB per short memory (row + FTS), plus embedding
bytes in `mem_vec` once sqlite-vec is loaded (not measured here;
measure again after changing embedding dims).

## Concurrency limits

SQLite allows many readers but one writer. Measured with 20 threads,
each holding a write txn ~50 ms, WAL mode:

- `busy_timeout=0`: 1 ok, 19 fail with `SQLITE_BUSY: database is locked`.
- `busy_timeout=5000`: 20 ok, 0 fail; losers wait transparently.

```sh
sqlite3 conc.db "PRAGMA journal_mode=WAL;"
# in each writer, before writing:
# PRAGMA busy_timeout=5000;
```

`crates/remem-graph` sets a 5 s busy timeout; set the same on every
store connection that shares a file. Keep write txns short; a writer
holding `BEGIN IMMEDIATE` blocks all other writers for its duration.
No multi-process write pooling exists: one process, short txns.

## File permissions

Memories may hold secrets. Restrict on creation:

```sh
umask 077
touch memories.db
chmod 600 memories.db
```

`remem-mcp` chmods the DB file and `-wal`/`-shm` sidecars to `0600`
(best effort, Unix) after open, because WAL sidecars inherit the umask,
not the DB mode. Verify:

```sh
ls -l memories.db*   # expect -rw------- on each present file
```
