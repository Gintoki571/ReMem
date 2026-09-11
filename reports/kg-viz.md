# kg-viz: `remem graph --html <file>` self-contained knowledge-graph visualization

Base: 92f421a (v3.1). Lane lane-kgui. DONE.

## What landed
1. `remem-graph::Graph::edges()` — whole-graph snapshot (VizNode{id,kind},
   VizEdge{from,rel,to}) via graphqlite Cypher `MATCH (n)` + `MATCH (a)-[r]->(b)`.
   TDD: RED 3f9f8f4, GREEN 748fcdf (graph suite 24/24).
2. `remem graph --html <file>` — crates/remem-recall/src/html.rs (102 lines) +
   CLI subcommand. Self-contained HTML: embedded JSON, <canvas> spring/repulsion
   layout, click node -> id/kind/content snippet, kind filter chips, no CDN, no
   server. Test: self-containment asserts (no http/script-src, canvas, filter).
3. AGENTS.md: resolved committed conflict markers in CLI section; documented
   `graph --html`.

## Verification (live)
- `cargo test --workspace` green; `cargo fmt` clean (fmt run per commit).
- Dogfood REMEM_DB=~/.remem/remem.db: exported 97 nodes / 129 edges, 37 KB,
  ~0.35 s total. Kind mix: fact 40, decision 36, mistake 10, Agent 8, event/note/Session 1.
- node --check on embedded JS: OK. Headless node run (DOM stubs): 200 layout
  ticks, 0 nodes out of bounds, click handler runs.

## Notes / friction (dogfood)
- Every CLI invocation pays model load + warm-up (~6 s steady-state on this DB
  for recall-type ops; `graph` itself is fast — no embedding needed). Confirms
  the daemon-latency motivation queued next.
- graphqlite has no cheap "return all edges with type" API (get_all_edges drops
  the rel type), hence edges() goes through cypher() with type(r)/labels().
- Snippet cap 200 chars to keep the file small; ids shown as 8-char prefix.

## Not done (YAGNI)
- Zoom/pan/drag, edge filtering, rel-type colors: single click-to-inspect +
  kind filter covers the stated need. Add on request.
