# Remem AGENTS.md

Local-first long-term memory engine for AI agents. Rust (v3) is main since
a9d18f6 retired the TypeScript tree; v2 is retired and kept only as
README-v2.md.

Docs: `docs/architecture-v3.md` (design), `docs/ops.md` (build/run). Read both before changing code.

### Skills
- Memory protocol: `skills/remem/SKILL.md` — dogfood remember/recall.
- Worker contract: `skills/tdd-worker/SKILL.md` — failing test first, then fix.

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

Local gate: `.git/hooks/pre-commit` runs `cargo fmt --check` (not tracked by git; reinstall if hooks dir reset).
CI enforces fmt + clippy + test on every push.
Always verify all three green before pushing.

TDD: add/update the failing test first, then the fix; never commit a red workspace.

### Layout (6 crates)

- `crates/remem-types` - shared domain types (MemoryKind/Item, RecallQuery/Hit). Additive changes only.
- `crates/remem-store` - rusqlite bundled + sqlite-vec **=0.1.9** (0.1.10-alpha.4 does not compile: missing sqlite-vec-diskann.c). Schema in `schema.sql`.
- `crates/remem-graph` - graphqlite 0.8 over the same file. Hub ids `agent:`/`session:` namespaced; `neighbors()` returns Memory mids only.
- `crates/remem-embed` - candle BERT (cadet-embed-base-v1), GPU via `--features cuda`, CPU fallback default. Tests skip when model dir absent (CI).
- `crates/remem-recall` - RRF fusion + `remem` binary. Real embedder wired in `main.rs` with stub fallback.
- `crates/remem-mcp` - MCP server (stdio) over the same engine. 11 tools: remember, recall, list, link, forget, purge, stats, validate, related, central, path.

### CLI (`./target/debug/remem`, global `--db`, `$REMEM_DB`)

- `remember <kind> <text> [--tags t1,t2] [--agent a] [--session s] [--importance f] [--occurred-at secs|YYYY-MM-DD]`
- `recall <query> [--k n] [--json] [--agent a] [--session s] [--since x] [--until y] [--max-chars n] [--min-score f] [--no-recency] [--half-life-days d]`
- `list [--json]` | `link <from> <to> [--rel r]` | `forget <id>` | `purge <id>` | `stats` | `validate` (exit 1 if dangling/orphan) | `related <id>` | `central` | `path <from> <to>` | `trace <id>` (correction chain, oldest->newest; empty if unknown)
- `graph --html <file>` — export the whole knowledge graph as a self-contained HTML file (canvas spring layout, click node for content, kind filter; no CDN/server; data via remem-graph `Graph::edges()`)

### Tests per crate

```bash
cargo test -p remem-types -p remem-store -p remem-graph -p remem-recall -p remem-mcp
cargo test -p remem-embed                  # skips without model dir
cargo test -p remem-embed --features cuda  # GPU path (needs CUDA 12.x; broken on 13.x, see docs/cuda-unblock.md)
```

### Agent memory protocol (dogfood v3)

```bash
export REMEM_DB="$HOME/.remem/remem.db"
./target/debug/remem remember <kind> "<content>" --tags t1,t2 --agent <name> --importance 0.8
./target/debug/remem recall "<query>" --k 5 [--agent x] [--session y]
```
