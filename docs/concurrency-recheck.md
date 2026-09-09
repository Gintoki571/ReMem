# Concurrency recheck (2026-09-08)

Re-run of docs/ops.md's contention probe against the explicit 5 s busy_timeout
(store sets it since `feat(v3): store busy_timeout and limit clamps`).

## Setup

- Binary: debug `remem` (`cargo build -p remem-recall`), embedder Cpu 768d.
- Scratch db: /tmp/conc-recheck.db (fresh).
- 20 parallel `remem remember` writers, unique content; then 10 parallel
  `remem recall` readers during a held write txn (sqlite3 BEGIN IMMEDIATE).

## Results

| probe | ok | busy/locked |
|---|---|---|
| 20 parallel writers | 20 | 0 |
| 10 parallel readers during held write txn | 10 | 0 |
| 1 writer vs txn held 28.8 s (> 5 s timeout) | 0 | 1 (clean exit 1, "database is locked") |

Post-run: 22 rows landed, integrity_check ok, WAL mode, no corruption.

## Verdict

The explicit busy_timeout holds under the real binary for realistic contention
(short write txns serialize transparently). A lock held longer than 5 s still
fails loudly with SQLITE_BUSY — timeout is backpressure, not a cure.
vs docs/ops.md baseline (1 ok / 19 SQLITE_BUSY at busy_timeout=0): fixed.
