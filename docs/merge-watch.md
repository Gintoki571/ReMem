# Merge watch (v3, read-only recon)

Date: 2026-09-08. Source: `git diff` working tree vs HEAD (branch v3).
Siblings uncommitted; this file tracks their inventories only.

## qwen — score floor (partially landed)

| File | Status | Functions / items |
|---|---|---|
| `crates/remem-recall/src/rank.rs` | LANDED (uncommitted) | `apply_floor(hits, min_score)` + 4 tests (`floor_drops_only_hits_below_it`, `floor_zero_is_off_and_keeps_everything`, `floor_above_top_hit_returns_empty_even_for_the_top`, `floor_on_empty_input_is_empty`); helpers `scored`, `floored_ids` (test-only) |
| `crates/remem-types/src/lib.rs` | NOT YET | `min_score` field on `RecallQuery` absent; only `max_chars: Option<usize>` present |
| `crates/remem-recall/src/main.rs` | NOT YET | `--min-score` flag absent; only `--max-chars` present |
| `crates/remem-recall/src/lib.rs` | NOT YET | no `apply_floor` wiring; tail is still `max_chars -> pack_by_budget / hits` only |

## glm — graph algos (landed)

| File | Status | Functions / items |
|---|---|---|
| `crates/remem-graph/src/lib.rs` | LANDED (uncommitted) | `Graph::central()`, `Graph::shortest_path(from, to)`, helper `hub_id_is_memory()` |
| `crates/remem-graph/tests/graph.rs` | LANDED (uncommitted) | `central_ranks_star_center_first`, `central_excludes_hubs`, `shortest_path_returns_full_route`, `shortest_path_unknown_ids_are_empty_not_errors` |

## Packing baseline (already in HEAD, not a sibling diff)

`pack_by_budget` + `max_chars` plumbing committed (27d1841, 0688658): `rank.rs::pack_by_budget`, `RecallQuery::max_chars`, `main.rs --max-chars`, `lib.rs` budget tail.

## Coexistence checks

- `--max-chars` vs `--min-score` in `main.rs`: NO clash today; `--min-score` does not exist yet. When qwen adds it, must be a separate `#[arg(long)]` name.
- `RecallQuery`: has `max_chars` only; `min_score` still to add alongside it (no duplicate).
- Same-function-differently: NONE. `rank.rs` diff is purely additive (`apply_floor` appended after `final_score`; `pack_by_budget` untouched). `remem-graph` diff is purely additive (new impl block items + helper). No overlapping hunks across siblings.

## Verdicts

- `rank.rs` (qwen floor vs packing baseline): GO — additive `apply_floor`, `pack_by_budget` untouched.
- `crates/remem-types/src/lib.rs` + `crates/remem-recall/src/main.rs` + `crates/remem-recall/src/lib.rs` (qwen remainder): CAREFUL — `min_score` field, `--min-score` flag, and floor-then-pack wiring order still to land; wire as floor before pack.
- `crates/remem-graph/src/lib.rs` + `crates/remem-graph/tests/graph.rs` (glm algos): GO — isolated new methods/tests, no overlap with qwen.
