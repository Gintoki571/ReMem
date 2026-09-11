# kg-viz: `remem graph --html <file>` self-contained knowledge-graph visualization

Base: 92f421a (v3.1). Lane lane-kgui.

## Plan
1. [x] TDD `Graph::edges()` in remem-graph — RED committed 3f9f8f4, GREEN 748fcdf (24 tests pass).
2. `graph --html <file>` subcommand in remem CLI: stdlib HTML/JS/CSS, <canvas>,
   spring layout, click node -> content, kind filter, no CDN, target <300 lines.

## Decisions
- `edges()` uses graphqlite Cypher (`MATCH (n)`, `MATCH (a)-[r]->(b)`) via the
  crate's existing `cypher()` — raw table schema is an extension internal, do
  not couple to it.
- Kind for hubs = label (`Agent`/`Session`); memory kind from `n.kind` property.

## Notes
- Root AGENTS.md has committed `<<<<<<< HEAD` conflict markers in the CLI
  section (related/trace lines) — fixing in this lane's DOX pass.
