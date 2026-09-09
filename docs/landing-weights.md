# Landing: learned recall weights (qwen)

Spec: RRF k=30, Weights fts 1.0 / vector 0.5 / graph 1.0,
importance off (reason only), recency kept narrow 0.9+0.1x,
tag boost capped 1.05x single factor. Predicts 35/37 @1
(docs/weight-spike.md row 1) vs 31/37 baseline.

## 1. Verify (order fmt -> clippy -> test -> eval)

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/eval.sh /tmp/remem-eval-weights.db
```

Single-signal check (fused must beat each arm alone):

```sh
# recall --k 40 --json on 37 answerable fixtures; recompute
# fts-only / vector-only / fused @1 from fts#N + vector#N reasons
# (method: docs/ranking-study.md); expect fused >= max(single).
```

Expected: fmt zero diff; clippy zero warnings; tests all green;
fused recall@1 >= 34/37; eval.sh (--min-score 0.02) recall@5 >= 35/37
with adversarial 3/3; fused >= each single-signal @1.

## 2. Red lines (any one forces REVISE, do not land)

- Any `cargo test --workspace` failure.
- Fused recall@1 below 31/37 baseline.
- `final_score` contains any importance term.
- `TAG_MATCH_BOOST` above 1.05x or stacked per hit.
- Recency band wider than 0.9+0.1x or importance re-added to pass eval.

## 3. Commit grouping (exact paths, one commit)

- `crates/remem-recall/src/rank.rs` (`DEFAULT_RRF_K`, `final_score`, boost)
- `crates/remem-recall/src/lib.rs` (`Weights::default`)
- `crates/remem-recall/` tests + this doc (`docs/landing-weights.md`)
- Never mix sibling paths (mcp/cli/graph/store); `git add` exact files only.
