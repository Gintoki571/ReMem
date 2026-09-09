# Fuzz report — `remem` DEBUG binary (v3 branch)

Build: `cargo build -p remem-recall` OK first try (binary is `target/debug/remem`, not `remem-recall`; `remem-mcp` binary is broken at startup — unrelated `no such column: content_hash` schema error, not fuzzed further). No staleness: binary rebuilt 22:04 UTC with siblings' uncommitted edits present. `forget` (soft-delete) and `purge` (hard-delete) both exist. Env: `REMEM_DB=/tmp/remem-fuzz.db` (fresh). No PANICs, no HANGs in 40 cases; verdicts below are OK (clean exit/error) vs logic bugs (exit 0 but corrupt/ignored data — still "OK" per the panic/hang scale, flagged as BUG).

## Results

| input | result | verdict |
|---|---|---|
| `remember fact ""` (empty text) | exit 0, stored empty-content row `ae00cacd` | OK (BUG: empty memory accepted) |
| `remember fact` (no TEXT arg) | exit 2 clap missing-arg error | OK |
| `remember fact <1MB single argv>` | OS `E2BIG` (arg list too long) — never reaches app | OK (env limit, not app bug) |
| `remember fact <100x10KB args ~1MB>` | exit 0, stored `69125bcc` (~18s embed, slow but completes) | OK |
| `remember fact <200KB single argv>` | OS `E2BIG` | OK |
| `remember bogus "some text"` | exit 1 `unknown kind 'bogus'` | OK |
| `remember fact ... --importance 999` | exit 0, stored `importance=999.0` (no clamp) | OK (BUG #1) |
| `remember fact ... --importance=-5` | exit 0, stored `importance=-5.0` (bare `-5` is a clap parse error; `=` form bypasses) | OK (BUG #1) |
| `remember fact ... --importance nan` | exit 0, dedup hit on content hash (no new row, silent no-op) | OK (BUG #1) |
| `remember fact ... --importance inf` | exit 0, dedup hit (same as nan) | OK (BUG #1) |
| `recall ""` (empty query) | exit 0, embedder runs, returns hits | OK (BUG: empty query returns data) |
| `recall` (no QUERY arg) | exit 2 clap missing-arg error | OK |
| `recall hello --k 0` | exit 0, returns full hits (k ignored) | OK (BUG #2) |
| `recall hello --k 999999999` | exit 1 clean `k value in knn query too large ... limit is 4096` | OK |
| `recall hello --k=-1` | exit 2 clean clap `invalid digit found in string` | OK |
| `recall hello --min-score nan` | exit 0, `[]` (NaN comparison drops everything, silent) | OK (BUG #3) |
| `recall hello --min-score inf` | exit 0, `[]` | OK |
| `recall hello --max-chars 0` | exit 0, full results (budget ignored) | OK (BUG) |
| `recall hello --max-chars 999999999` | exit 0, full results | OK |
| `remember ... --occurred-at 999999-01-01` | exit 0, stored `occurred_at=31494753244800` (year 999999 accepted) | OK (BUG) |
| `remember ... --occurred-at garbage!!!` | exit 1 clean `invalid time` | OK |
| `remember ... --occurred-at=-12345` | exit 0, stored `occurred_at=-12345` (pre-epoch accepted; bare `-12345` is a clap error, `=` form bypasses) | OK (minor) |
| `remember ... --occurred-at 99999999999999` | exit 0, dedup hit (same content `time test`, occurred_at ignored on duplicate) | OK (BUG: occurred_at not part of dedup, silent drop) |
| `recall hello --since garbage!!!` | exit 1 clean `invalid time` | OK |
| `link "" ""` | exit 1 clean `graph link: node not found:` | OK |
| `link <10KB> <10KB>` | exit 1 clean `node not found: XXX...` (truncated in msg) | OK |
| `link 'a\'"; DROP TABLE...' 'b...' ` | exit 1 clean `node not found`, tables intact (`memories=7, edges=0`) | OK |
| `purge ""` | exit 1 clean `unknown id ''` | OK |
| `purge <10KB>` | exit 1 clean `unknown id 'ZZZ...'` | OK |
| `purge 'x\'"; DROP TABLE...'` | exit 1 clean `unknown id`, tables intact | OK |
| `purge does-not-exist-123` | exit 1 clean `unknown id` | OK |
| `REMEM_DB=/dev/null remember/recall/stats` | exit 1 clean `open store: SQLITE_READONLY_DIRECTORY` | OK |
| `REMEM_DB=/nonexistent-dir-xyz/db.sqlite` | exit 1 clean `unable to open database file` (code 14) | OK |
| `REMEM_DB=<non-UTF8 path>` | exit 2 clean clap `invalid UTF-8` | OK |

Crash count: **0 panics, 0 hangs** (40 cases; slowest was ~1MB embed ~18s, completes).

## Top 3 to fix

1. **`--importance` unvalidated (999/-5/nan/inf all exit 0)** — 999.0 and -5.0 persisted to DB (verified via SQL); nan/inf silently dedup-hit. Rank scoring trusts this column (`important` reason on 999 row). Fix: clamp/reject to [0,1], reject non-finite.
2. **`recall --k 0` returns hits** — k=0 should return `[]`; it returns the full ranking, so any caller paging with k=0 gets data. Fix: early-return empty or reject k=0.
3. **`--min-score nan` silently returns `[]`** — NaN poisons the score filter so every hit is dropped with exit 0. Indistinguishable from "no matches". Fix: reject non-finite min-score (same validation as #1).
