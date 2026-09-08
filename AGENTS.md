# Remem AGENTS.md

Local-first long-term memory engine for AI agents. TypeScript, Node 22+.

## Branches

- `main`: legacy v1 engine (do not modify)
- `v2`: active development (this branch)

## Build & Test

```bash
npm install
npm run typecheck   # tsc --noEmit
npm test            # vitest run
npm run lint        # biome check .
```

All three must pass before committing. CI enforces them on every push.

## Architecture (v2)

- `src/types.ts` - domain types + zod schemas
- `src/store.ts` - SQLite via node:stdlib `node:sqlite` (WAL, FTS5, trigger-maintained index, embeddings as BLOB)
- `src/embedder.ts` - ONNX embeddings via transformers.js; device auto = webgpu -> cpu. Model at `models/onnx/model.onnx` (gitignored; re-export via `scripts/export-onnx.py` from /home/bindesh/rag/cadet-embed-base-v1)
- `src/recall.ts` - RRF fusion of vector + FTS rankings, recency + importance weighting
- `src/graph.ts` - LatticeDB graph (@hajewski/latticedb): memory nodes, agent/session hub edges, HNSW vectors, durable event streams
- `src/remem.ts` - facade: remember / recall / link / events
- `src/cli/cli.ts` - CLI (also the agent integration surface)

## LatticeDB gotchas

- Node ids are bigint internally; `getNode`/`getProperty` take bigint, not string.
- `findNodesByLabelProperty` fails on string property values; use cypher `MATCH` lookups.
- Never call `txn.commit()` inside `db.write()` callback (auto-commits; double commit throws).
- `vectorDimensions` is baked into the db file at creation.

## Agent memory protocol (dogfood)

Agents working in this repo must use remem itself:

```bash
export REMEM_DATA_DIR="$HOME/.remem-agent"
npx tsx src/cli/cli.ts remember <kind> "<content>" --tags <tags> --agent <your-name>
npx tsx src/cli/cli.ts recall "<query>" --k 5
```

Save: non-obvious bug fixes (mistake), design rationale (decision), environment facts (fact). Recall before touching unfamiliar code. See skills/remem/SKILL.md.

## v3 (Rust, this branch family)

Branches: `main` = legacy v1 (do not modify). `v2` = TypeScript engine. `v3` = Rust engine (active).

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

All three green before commit. CI enforces fmt + clippy + test on every push.

### Layout

- `crates/remem-types` - shared domain types (MemoryKind/Item, RecallQuery/Hit). Additive changes only.
- `crates/remem-store` - rusqlite bundled + sqlite-vec **=0.1.9** (0.1.10-alpha.4 does not compile: missing sqlite-vec-diskann.c). Schema in `schema.sql`.
- `crates/remem-graph` - graphqlite 0.8 over the same file. Hub ids `agent:`/`session:` namespaced; `neighbors()` returns Memory mids only.
- `crates/remem-embed` - candle BERT (cadet-embed-base-v1), GPU via `--features cuda`, CPU fallback default. Tests skip when model dir absent (CI).
- `crates/remem-recall` - RRF fusion + `remem` binary. Real embedder wired in `main.rs` with stub fallback.

### Agent memory protocol (dogfood v3)

```bash
export REMEM_DB="$HOME/.remem/remem.db"
./target/debug/remem remember <kind> "<content>" --tags t1,t2 --agent <name> --importance 0.8
./target/debug/remem recall "<query>" --k 5 [--agent x] [--session y]
```
