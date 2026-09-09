---
name: tdd-worker
description: Run a scoped TDD task in one crate directory. Failing test first with RED proof, minimal fix, suite green, short reply. Use for remem v3 worker briefs.
---

# TDD Worker

## 1. Scope and ownership

- Own exactly one crate or directory named in the brief. Read only what it touches first.
- Never edit outside scope. Never touch sibling crates or their docs.
- Never `git commit`, `git push`, `git reset`, `git checkout .`, or `git stash`.

## 2. TDD loop

1. Write the failing test first covering the brief acceptance case.
2. Run it, confirm RED, quote the failure output.
3. Write the smallest code that turns it green (ponytail: reuse helper, stdlib, existing dep).
4. Run the crate suite plus workspace check for touched areas. Fix regressions before replying.
5. Verify with `git status --short` and `git diff --stat` that only owned files changed.

## 3. Reply contract (<=25 lines)

- Files changed (paths only).
- Test output quoted: RED proof line + final green summary.
- Open questions, if any. No essays, no progress log.

## 4. Skill discipline

- Default to ponytail full: shortest diff, no speculative abstraction.
- Call `refine` when a reusable lesson appears (recurring failure, durable API fact, trap).
- Never document a silent-failure root cause without a reproduced RED test.

## 5. Stuck protocol

- Retry a failing step at most twice with new evidence each time.
- Then stop and report `BLOCKED`: what was tried, exact error quoted, files touched.
- Never thrash, never widen scope, never rewrite unrelated code to force green.
