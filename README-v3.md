# ReMem v3 (Rust)

Local-first long-term memory engine for AI agents. v3 is a Rust rewrite of the v2 TypeScript engine.

- `main` branch: legacy v1 engine (do not modify).
- `v2` branch: TypeScript engine.
- `v3` branch (this): Rust engine, active development.

Storage is a single SQLite file: relational tables + FTS5 keyword index + sqlite-vec vector index + graphqlite graph projection, all in the same file.

## Crate layout

- `crates/remem-types` - shared domain types (`MemoryKind`, `MemoryItem`, `RecallQuery`, `RecallHit`). Additive changes only.
- `crates/remem-store` - SQLite storage via rusqlite (bundled) + sqlite-vec `=0.1.9`. Schema lives in `crates/remem-store/schema.sql`. Do not use sqlite-vec 0.1.10-alpha.4 (fails to compile: missing `sqlite-vec-diskann.c`).
- `crates/remem-graph` - graphqlite 0.8 graph over the same DB file. Hub ids are namespaced (`agent:<name>`, `session:<name>`); `neighbors()` returns memory mids only.
- `crates/remem-embed` - local BERT embeddings (cadet-embed-base-v1) via candle. Mean-pool + L2 norm, 768 dims. CPU by default, CUDA via feature flag.
- `crates/remem-recall` - RRF fusion of vector + FTS rankings (plus recency/importance weighting) and the `remem` binary (`src/main.rs`). Uses the real local embedder when the model dir is present, stub embedder fallback otherwise.

## Build and test

```bash
cargo build
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

All three of test, clippy, and fmt must pass before committing.

## CLI reference

Binary: `./target/debug/remem`. Global flag `--db <path>` (env `REMEM_DB`, default `~/.remem/remem.db`).

| Command | Usage |
|---|---|
| `remember` | `remem remember <kind> <text...> [--tags t1,t2] [--agent NAME] [--session NAME] [--importance 0.5] [--occurred-at SECS_OR_YYYY-MM-DD]` |
| `recall` | `remem recall <query...> [--k 5] [--json] [--agent NAME] [--session NAME] [--since SECS_OR_YYYY-MM-DD] [--until SECS_OR_YYYY-MM-DD]` |
| `list` | `remem list [--json]` (newest first) |
| `link` | `remem link <fromId> <toId> [--rel REL]` |
| `forget` | `remem forget <id>` (soft-delete: row kept, graph node and edges dropped) |
| `purge` | `remem purge <id>` (hard-delete row, FTS entry, embedding, graph node) |
| `related` | `remem related <id> [--rel REL]` (neighbours as `id  rel`, Memory-only, sorted) |
| `central` | `remem central [--limit 10]` (PageRank as `id  score`, desc) |
| `path` | `remem path <from> <to>` (shortest path one id per line, empty if unreachable) |
| `stats` | `remem stats` (JSON counts) |
| `validate` | `remem validate` (dangling edges + orphans, exit 1 if any) |

Kinds: `fact | decision | mistake | preference | event | note`. `list` takes no `--k`; `link` takes `--rel` (not `--type`). MCP tools mirror the 11 CLI commands one-to-one (full CLI/MCP parity); `forget` is soft, `purge` hard. Unknown ids in `forget`/`related`/`path` give no output, not an error.

### Examples

```bash
export REMEM_DB="$HOME/.remem/remem.db"

# 1. Save a memory
./target/debug/remem remember mistake "sqlite-vec 0.1.10-alpha.4 does not compile, pin 0.1.9" \
  --tags sqlite-vec,rust --agent prime --importance 0.9

# 2. Recall by meaning (top 5, JSON output)
./target/debug/remem recall "which sqlite-vec version builds" --k 5 --json

# 3. Recall filtered to one agent + session + time window, then validate
./target/debug/remem recall "graph hub ids" --k 5 --agent prime --session v3-docs
./target/debug/remem recall "sqlite-vec build" --k 5 --since 2026-08-01 --until 2026-09-08
./target/debug/remem validate && ./target/debug/remem stats

# 4. Backdate an event and use the MCP tools (forget by id, validate for health)
./target/debug/remem remember event "cut v3 release" --occurred-at 2026-09-01 --agent prime
```

## Quality

40-fixture eval (`docs/eval.md`, runner `scripts/eval.sh`, final 2026-09-08):
answerable queries unfloored recall@1 31/37 (84%), recall@5 37/37 (100%).
3 adversarial pure-stopword queries have no good answer and return near-zero-score junk unfloored.
`--min-score 0.02` floor stays opt-in (default off): floored recall@1 31/37, recall@5 35/37, suppresses all 3 adversarial queries at the cost of the two weakest rank-5 hits (Q27/Q28); see `docs/floor-decision.md`.
25-fixture baseline was recall@1 4/25 (16%), recall@5 16/25 (64%).

## GPU / CUDA note

Default build is CPU-only and always works. CUDA is opt-in:

```bash
cargo build --features cuda         # crate remem-embed exposes feature `cuda`
cargo test --workspace --features cuda
```

Runtime device selection is `Device::cuda_if_available`, with CPU fallback. The model dir defaults to `/home/bindesh/rag/cadet-embed-base-v1`, overridable with `REMEM_EMBED_MODEL_DIR`. The GPU smoke test is `#[ignore]`d; run it explicitly on a CUDA host with `cargo test -p remem-embed --features cuda -- --ignored`.

## CI behavior

`.github/workflows/ci.yml` runs two jobs on every push/PR: a Node job (typecheck + lint + vitest) and a Rust job (`cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`).

Embedder tests skip (pass) when the model dir is absent: each test returns early after printing `skip: no local model at ...`. So CI without the model weights stays green. The CLI also never fails for a missing model: `main.rs` prints `embedder: local model unavailable (...), using stub` and recalls with the stub embedder.
