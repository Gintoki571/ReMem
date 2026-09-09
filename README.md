# ReMem — long-term memory for AI agents

ReMem is a local-first memory engine: agents store facts, decisions, mistakes,
preferences, and events, then recall them by meaning. One SQLite file holds
everything. No server, no API keys, no network.

```sh
cargo build
./target/debug/remem remember fact "production db runs Postgres 16"
./target/debug/remem recall "which database is production on"
# -> production db runs Postgres 16  (score + reasons: fts/vector/recent/...)
```

## How it works

One SQLite file (`~/.remem/remem.db` by default) holds four things at once:

- **Tables** — memories with kind, tags, agent, session, timestamps, importance.
- **FTS5** — keyword index with porter stemming plus prefix arms for stem gaps
  (`auditors` finds `audit`).
- **sqlite-vec** — vector index over local 768-dim embeddings; nearest-meaning
  match by cosine distance.
- **Graphqlite** — the same rows projected as a graph, queried with Cypher
  (`related`, `central`, `path`).

A recall runs keyword search and vector search in parallel, fuses the two
rankings (RRF + recency/importance), then applies a gated tag boost and a
score floor that drops junk. Measured on 40 fixtures: **35/37 top-1, 37/37
top-5**, and 3/3 adversarial queries correctly return nothing.

## Install

Requires Rust stable. Embeddings run on CPU out of the box.

```sh
git clone https://github.com/Gintoki571/ReMem && cd ReMem
cargo build --release
export REMEM_DB="$HOME/.remem/remem.db"
./target/release/remem stats
```

Embeddings need a BERT model dir (compatible with cadet-embed-base-v1).
Set `REMEM_EMBED_MODEL_DIR` to it; without one the binaries print a warning
and run on a stub embedder (keyword search still works, vector arm does not).

## CLI

Binary: `remem`. Global `--db <path>` / `REMEM_DB` (default `~/.remem/remem.db`).

| Command | What it does |
|---|---|
| `remember <kind> <text>` | Store a memory (`fact\|decision\|mistake\|preference\|event\|note`), with `--tags`, `--agent`, `--session`, `--importance`, `--occurred-at` |
| `recall <query>` | Top hits with scores + reasons (`--k`, `--json`, `--min-score`, `--agent/--session/--since/--until` filters) |
| `list` | Newest first (`--limit`, `--json`) |
| `link <a> <b>` | Connect two memories (`--rel`) |
| `forget <id>` | Soft-delete (kept, hidden from recall) |
| `purge <id>` | Hard-delete |
| `related \| central \| path` | Graph neighbours / PageRank / shortest path |
| `stats \| validate` | JSON counts / integrity check (exit 1 on dangling edges) |

Full tour with expected output: `bash scripts/demo.sh` (12 steps, ~25s).

## MCP server

`remem-mcp` exposes the same 11 commands over newline-delimited JSON-RPC on
stdio — one-to-one CLI/MCP parity. Point any MCP client at the binary with
`REMEM_DB` set. Handshake: `initialize` → `notifications/initialized`
(no reply) → `tools/list`.

## Development

```sh
cargo test --workspace          # all suites green before push
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check               # enforced by a pre-commit hook
bash scripts/eval.sh            # 40-fixture recall eval
```

Details live in `docs/`: `eval.md` (numbers), `demo.md` (tour),
`architecture.md` + `diagrams/` (design), `merge-execution.md`,
`stemmer-analysis.md`, `rank1-roadmap.md`.

## Project layout

- `crates/remem-types` — shared domain types.
- `crates/remem-store` — SQLite + FTS5 + sqlite-vec (`=0.1.9`; never the
  0.1.10-alpha, it does not compile).
- `crates/remem-graph` — graphqlite 0.8 graph over the same file.
- `crates/remem-embed` — local embeddings, CPU (CUDA blocked upstream, see below).
- `crates/remem-recall` — fusion ranking + `remem` CLI.
- `crates/remem-mcp` — `remem-mcp` stdio server.
- `skills/remem/` — agent skill: the store/recall protocol for AI users.

## History

- **v3 (this branch, Rust)** — active. SQLite + sqlite-vec + graphqlite.
- **`v2` branch** — older TypeScript engine (LatticeDB + transformers.js).
  Kept for reference; see `README-v2.md`.

## GPU / CUDA

CPU-only, by design for now. GPU embeddings via candle are blocked upstream:
`candle-kernels` does not ship kernels for CUDA 13.x (`sm_75` arch), and the
upstream fix is unmerged — so enabling the `cuda` feature fails at link time
on current toolchains. Tracked in issue #5. If you have CUDA ≤ 12.6 the
feature path may build; otherwise CPU is the supported path.

## License

MIT.
