# Hub-edge toggle in `graph --html` export (lane-vizhub)

## Task
`remem graph --html <file> [--with-hubs]`: agent:/session: hub nodes and
BELONGS_TO_* edges excluded by default (user screenshot: hub spokes drown
semantic edges); `--with-hubs` restores them. Hub-node count in summary line.

## Plan (TDD RED-first)
1. [x] Orientation: export lives in crates/remem-recall/src/main.rs Cmd::Graph; VizNode/VizEdge snapshot from crates/remem-graph Graph::edges (includes hubs).
2. [ ] RED: CLI test — export default has no `agent:`/`session:` nodes or BELONGS_TO_* edges; --with-hubs has them.
3. [ ] GREEN: `--with-hubs` flag + filter + hub count in summary line.
4. [ ] cargo test + fmt green, commit.

## Findings
- (in progress)

## Dogfood notes / friction
- (pending)
