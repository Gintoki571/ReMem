# Landing checklist (v3): qwen CLI + glm MCP on tuple engine

HEAD engine API (committed): `RecallEngine::remember(&item) -> (String, Vec<(String, f32)>)`. Both siblings destructure the tuple; neither changes it.

## 1. Preconditions

- qwen owns ONLY `crates/remem-recall/src/main.rs` (+ `crates/remem-recall/tests/cli.rs` hunk `cli_forget_related_central_and_path`). Reply must paste: new `Cmd::{Forget, Related, Central, Path}` variants, dispatch bodies, printer shapes (related `"<id>  <REL>"` sorted; central `"<id> <score>"` desc; path one id/line, empty when unreachable; forget/unknown-ids exit 0 empty).
- glm owns ONLY `crates/remem-mcp/src/main.rs`. Reply must paste: `let (id, similar) = eng.remember(&item)?` line + `similar: [{id, distance}]` mapping. No engine-file edits from either.
- Both rebased on HEAD `306434c`; dirty `tests/cli.rs` hunk stays uncommitted until qwen lands (do NOT commit it early).

## 2. Order

1. glm first: one file, already tuple-shaped, asserts the engine contract; smallest blast radius.
2. qwen second: new user surface (4 subcommands), needs full CLI test run after engine contract is proven green.

## 3. Verify (each landing; expected: all green, zero output = pass for fmt)

- `cargo fmt --all -- --check` (expect: no diff).
- `cargo clippy --workspace --all-targets -- -D warnings` (expect: no warnings).
- `cargo test --workspace` (expect: all pass, incl. `cli_forget_related_central_and_path` after qwen).
- `REMEM_DB=/tmp/remem-demo.db scripts/demo.sh` (expect: exit 0).
- Mirror: `git -C /tmp/remem-head fetch origin && git -C /tmp/remem-head reset --hard <HEAD>`; confirm `rev-parse HEAD` matches root; apply landing patch there; rerun the four commands above (expect: same green). /tmp/remem-head is STALE now (9db846b vs 306434c) — refresh before any landing.

## 4. Commits + push + CI

- Commit 1 (glm): ONLY `crates/remem-mcp/src/main.rs` (+ its test if any). Commit 2 (qwen): ONLY `crates/remem-recall/src/main.rs` + `crates/remem-recall/tests/cli.rs`. Never mix sibling paths in one commit; never touch `crates/remem-recall/src/lib.rs` (tuple owner).
- `git add <exact paths>; git commit -m "..."; git pull --rebase; git push`. Then watch CI to green on the pushed SHA before the next landing.

## 5. Rollback

- STOP-AND-REVERT (`git revert <sha>`): workspace tests fail on HEAD-mirror too; tuple signature touched; CLI/MCP output contract broken vs parity doc; /tmp/remem-head SHAs diverge.
- FIX-FORWARD (same commit): fmt/clippy-only failure; flaky single test passing on mirror retry.
- Top trigger: red workspace tests after either landing = revert first, diagnose second.
