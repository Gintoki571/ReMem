# Graph review 2 — `crates/remem-graph/src/lib.rs`

Scope: hub namespacing, `neighbors()` filtering, dangling edges, `central()`/`shortest_path()` failures, Cypher injection. Read-only review; no code touched.

## Findings

| # | Severity | Location | Issue | Evidence |
|---|----------|----------|-------|----------|
| 1 | Medium | `upsert_memory` / `upsert_agent` / `hub_id_is_memory` (`central`) | A memory id starting with `agent:` or `session:` collides with a hub node id in graphqlite's single id space, and `central()` then drops that memory (prefix filter, not label check). | `upsert_memory` uses the raw id as node id; `upsert_agent("x")` uses node id `agent:x`; `central()` keeps only `hub_id_is_memory` (prefix test). |
| 2 | Low | `shortest_path` | Documented as "memory-to-memory" but does not filter hubs: hub endpoints and hub intermediaries can appear in paths, unlike `neighbors()` which excludes hubs. | `shortest_path` returns graphqlite Dijkstra `sp.path` unfiltered; no `MEMORY_LABEL` check. |
| 3 | Low | `neighbors` / `neighbors_detail` | Unknown node id returns empty vec, not `MissingNode` — inconsistent with `link()`, so typos fail silently. | `neighbors_detail` `MATCH (n {id: $id})` yields zero rows for unknown ids; no existence check. |
| 4 | Low | `link` | Existence pre-check is check-then-act across connections: a node deleted between `has_node` and `upsert_edge` can still dangle or error. | `link()` loops `has_node` then calls `link_unchecked`; two writers share one file (WAL + busy timeout). |
| 5 | Low | `neighbors_detail` row decoding | `unwrap_or(-1/-2/default)` on `nid`/`src`/`rel` masks graphqlite column changes as silently wrong `outgoing`/`rel` instead of an error. | `row.get("nid").unwrap_or(-1)`, `row.get("src").unwrap_or(-2)`, `row.get("rel").unwrap_or_default()`. |
| 6 | Low | `neighbors_detail` id resolution | A neighbour with none of `mid`/`aid`/`sid` (e.g. node created via raw `cypher`) surfaces as id `""`. | `.find_map(...).unwrap_or_default().to_string()` over `mid`/`aid`/`sid`. |
| 7 | Low | `shortest_path` self-path | `shortest_path(x, x)` behaviour is undocumented; result depends entirely on graphqlite Dijkstra self-path semantics. | `Ok(match sp.found { true => sp.path, false => vec![] })` with no same-endpoint handling. |
| 8 | Info | `validate` early return | If the dangling-edges query fails, `validate` returns immediately and skips the orphan check (partial health report). | `Err(e) => return vec![format!("validation query failed: {e}")]` vs `push` in the orphan branch. |
| 9 | Info | `link` rel-type normalisation | Non-identifier characters in `rel` are silently rewritten to `_` by graphqlite, so the stored type can differ from the input spelling. | Doc comment on `link()` states the rewrite; no error or normalised-value return. |
| 10 | Info | `cypher` / `neighbors_detail` params | No injection surface: all caller values are bound via `.param()` / `.params()`; `format!` SQL only interpolates crate constants. | `cypher_builder(...).param("id", mid)`, `query_builder(query).params(...)`, static SQL in `dangling_edges`. |

## Test coverage per finding

1. Not covered: `hub_id_cannot_collide_with_a_memory_id` uses bare `"shared"` only; no test with a prefixed memory id or `central()` on one.
2. Not covered: no `shortest_path` test involves a hub node.
3. Not covered: no test asserts `neighbors`/`neighbors_detail` on an unknown id (unknown-id tests exist only for `shortest_path`).
4. Not covered: no concurrent delete-during-`link` test.
5. Not covered: no test on decoding failure / renamed columns.
6. Not covered: all tests create nodes via typed APIs, never a bare node without `mid`/`aid`/`sid`.
7. Not covered: no self-path test.
8. Not covered: failure-path tests only cover healthy/dangling/orphan rows, never a broken query.
9. Not covered (behaviour documented in code, no test pins the rewrite).
10. Covered: `cypher_match_roundtrip_with_params` and `cypher_rejects_non_object_params` exercise bound params and rejection.
