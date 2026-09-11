# report: `remem impact <id>` (issue #16)

## Status
- [x] RED tests committed (0dddcdf)
- [x] graph BFS primitive (08feb82)
- [x] store successor/get_any (08feb82)
- [x] CLI subcommand + tree output (d618267)
- [x] cargo test/fmt green (workspace, no-cuda feature set)
- [x] dogfood notes (below)

## Design
(donor: graphify/affected.py:190 affected_nodes — BFS over incoming edges, depth-limited, rel-filtered)

- `remem_graph::Graph::impact(seed, depth, rel) -> ImpactReport { nodes: Vec<ImpactNode> }` — directional
  BFS from the seed over BOTH directions of every edge (dependents and dependencies),
  SUPERSEDES correction chains included. Each node carries `depth`, first-hop `rel`, and edge
  `provenance` (`correction-chain` / `manual`).
- Engine wrappers: `impact_depth(id, depth)` (full), `impact(id)` (depth 3 default),
  `impact_filtered(id, depth, rel)` (single relation). Unknown id -> empty report, no error.
- `Store::get_any(id)` — reads a memory row regardless of `superseded_at`/`ended`, so superseded
  chain members show text in the tree.
- CLI: `remem impact <id> [--depth N] [--rel REL]` — root line `id  [kind]  snippet`, then one
  line per node, indented 2 spaces per hop: `<- id  [kind]  REL (provenance)  snippet`.
  Nodes sorted by (depth, id) for deterministic output. Snippet truncated at 80 chars.
- Live DB note: current production edges are only `BELONGS_TO_AGENT`/`BELONGS_TO_SESSION`
  (139 edges, no memory<->memory links yet); `impact` on the live DB correctly returns a bare
  root line for those. `correct` (SUPERSEDES) creates real impact chains.

## Dogfood (synthetic temp DB)
Chain `old <-correct- mid <-correct- new`, plus `dep1 -USES-> new`, `deep -USES-> dep1`:
- default depth 3: chain tail + first dependent shown; `deep` is 4 hops from `old`
  (`deep -> dep1 -> new -> mid -> old`) and stays hidden (predecessor finding confirmed; the
  TEST asserts the hide at depth 3 and reach at `--depth 4`, not a tree change).
- `--rel SUPERSEDES` drops the USES fan-out, keeps the correction chain.
- `impact <new>` (tip) walks back to superseded rows: both directions confirmed.

## Verification
- `cargo test --workspace`: 0 failed, 2 ignored, all suites green without the cuda feature.
- `cargo fmt --all -- --check`: clean.
- `cli_impact_tree` (tests/cli.rs): chain + fan-out, depth default/cut/explicit, rel filter,
  reverse walk from tip, unknown-id empty tree, indents per hop.

## Friction notes
- `cargo test --workspace` failed to COMPILE on this lane: `crates/remem-embed/examples/gemm_probe.rs`
  uses `cudarc` but has no `required-features`, so default-feature builds break (predates this lane).
  Fixed in d618267 by adding `[[example]] name = "gemm_probe" required-features = ["cuda"]` to
  remem-embed/Cargo.toml. `cuda_probe.rs`/`bench.rs` need no gate (no optional-dep imports).
- reports/impact.md was deleted from the worktree by commit ce8e889 (which intended only stray
  lane reports); restored from 0d8ecd7 and kept in-repo.
- Do NOT raise default depth to chase deeper chains — callers pass `--depth`.
