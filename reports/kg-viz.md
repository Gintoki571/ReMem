# kg-viz: `remem graph --html <file>` self-contained knowledge-graph visualization

Base: 92f421a (v3.1). Lane lane-kgui.

## Plan
1. TDD: `Graph::edges()` in remem-graph (all nodes + edges for viz) — RED first.
2. `graph --html <file>` subcommand in remem CLI; stdlib HTML/JS/CSS, canvas,
   force-ish layout, click-for-content, kind filter, no CDN, <300 lines.

## Progress
- [x] Read DOX chain (root AGENTS.md; found committed conflict markers — TODO fix)
- [ ] RED test edges()
- [ ] edges() impl green
- [ ] CLI + html export
- [ ] dogfood on real DB

## Notes
- (incremental)
