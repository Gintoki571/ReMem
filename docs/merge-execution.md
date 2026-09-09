# Merge v3 to main — execution checklist (human/root only, IF approved)

1. `git fetch origin` — GO: no error, no new output or updated refs.
2. `git rev-parse origin/main` — GO: `411c6d3db12546c727a79baba0f01e13af3e2195` (main unmoved).
3. `git rev-list --count v3..main` — GO: `0`; `git rev-list --count main..v3` — GO: `164`.
4. `git checkout main && git status --short` — GO: clean, empty output.
5. `git merge --no-ff v3 -m "merge(v3): remem v3 engine to main"` — GO: `Merge made by the 'ort' strategy.`
6. `git log --oneline -3 && git status --short` — GO: merge commit on top, clean tree.
7. `npm run typecheck && npm run lint` (optional pre-push gate) — GO: both exit 0.
8. `git push origin main` — GO: `main -> main`, no rejected hint.
9. `git tag -a v3.0 -m "remem v3.0" && git push origin v3.0` — GO: `* [new tag] v3.0 -> v3.0`.
10. Verify CI on main: `gh run list --branch main --limit 3` — GO: jobs `test`, `rust`, `demo` all `completed success`.
11. ROLLBACK (only if CI red): `git reset --hard origin/main~0` on a bad local merge BEFORE push; AFTER a bad push: `git reset --hard 411c6d3 && git push --force-with-lease origin main && git branch -D v3-merge-tmp` — then delete tag if pushed: `git push --delete origin v3.0 && git tag -d v3.0`.
12. Keep `v3` branch until step 10 is green; delete only after: `git branch -d v3 && git push origin --delete v3`.
