# remem v3 performance

Method: release binary, `REMEM_DB=/tmp/remem-perf.db` (205 memories, 3.4 MB
at measure time). Per-invocation wall time via `time.perf_counter` around
a fresh process spawn. 100x `remember`, 50x `recall --k 5`, 10x each for
`validate` / `stats`.

## Hardware

- CPU: Intel Core i7-9750H @ 2.60 GHz (12 logical cores, /proc/cpuinfo)
- GPU: NVIDIA GeForce GTX 1660 Ti (`nvidia-smi -L`)
- Embedder device (stderr): `embedder: Cpu (768d)` — GPU present but unused,
  embedding runs on CPU

## Binaries

- `target/release/remem`: 13,372,448 bytes (12.8 MB)
- `target/release/remem-mcp`: 12,752,080 bytes (12.2 MB)

## Latency (release binary)

| command | n | avg (ms) | p50 (ms) | min (ms) | max (ms) |
|---|---|---|---|---|---|
| `remember fact <text>` | 100 | 248.4 | 247.7 | 240.6 | 266.5 |
| `recall --k 5 <query>` | 50 | 241.1 | 240.9 | 236.2 | 252.1 |
| `validate` | 10 | 149.4 | — | 146.3 | 155.5 |
| `stats` | 10 | 150.0 | — | 147.5 | 152.0 |

## Bottleneck

Startup dominates, not SQLite. `stats` and `validate` do no embedding work
yet cost ~150 ms per invocation, and every command prints the
`embedder: Cpu` line — i.e. each CLI spawn pays process startup plus
embedder/SQLite setup before doing anything. `remember`/`recall` add only
~90–100 ms on top of that floor for one CPU embedding plus the actual
SQLite/vec0 write or query, with tight min–max spreads. So the single
biggest cost is per-process startup (embedder init on CPU), then the single
embedding pass; the SQLite query itself is negligible by subtraction.
A long-lived process (e.g. `remem-mcp`) that loads the embedder once would
remove the ~150 ms floor from every operation.
