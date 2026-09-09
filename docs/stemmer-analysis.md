# Stemmer analysis: porter unicode61 vs alternatives (GitHub #6, Q28)

Offline analysis only. No code changed. Method: scratch FTS5 tables in
sqlite3 (3.53.1 python module / 3.53.4 CLI, same porter as the production
schema), all 40 `docs/eval-fixtures.json` memories loaded into a 2-column
(content, tags) table, bm25 weights 1.0/2.0, and the real `fts_quote`
stopword-drop + OR-join logic from `crates/remem-store/src/lib.rs`
reimplemented in the harness. Fused-level predictions re-fuse the new FTS
lists against the real `remem recall --k 40` vector lists (RRF k=30,
fts 1.0 / vector 0.5, 0.9+0.1 recency, gated 2.0x tag boost, floor 0.017),
so the baseline reproduces the published 35/37 @1 and 35/37 @5 exactly.

## 1. Stem check (porter unicode61, via fts5vocab)

| word | porter stem | grouped with |
|------|-------------|--------------|
| audit | audit | auditing, audited, audits |
| auditors / auditor | auditor | (isolated) |
| run / running / runs | run | (one group) |
| ran | ran | (isolated, irregular) |
| decide / decided | decid | (isolated from decision) |
| decision / decisions / decisive | decis | (isolated from decide) |
| migrate / migration / migrating / migrated | migrat | one group, no divergence |
| token / tokens / tokenize / tokenized / tokenizer | token | one group, no divergence |

## 2. Divergent pairs (the Q28 class)

| pair | stems | in fixtures? |
|------|-------|--------------|
| auditors vs audit/auditing/audited | auditor vs audit | yes: Q28 query "auditors", doc 28 "audit" |
| decide/decided vs decision(s)/decisive | decid vs decis | yes: Q27 query "decide", doc 27 "decision" (tag-only anchor today) |
| deploy/deploying vs deployment(s) | deploi vs deploy | latent (both forms exist in corpus) |
| store/stored vs storage | store vs storag | latent |
| prioritize vs priority | priorit vs prioriti | latent |
| index/indexes vs indices | index vs indic | latent |
| run vs ran | run vs ran | not in fixtures |

migrate/migration and token/tokens do NOT diverge; the issue's "porter
stems differently" class is real but narrow (irregular -or/-ion/-y endings).

Q28 mechanism, verified: with the current index, `"auditors"` matches 0
docs; `"audit"` matches docs 11 and 28. The query never lands a lexical
hit on the target, so the target rides vector#1 alone and fuses to rank 4
at 0.0161 — under the 0.017 floor. (The issue title names auditors/audit
correctly; note Q28 as scored at the floor in docs/floor-decision.md is
the same root cause plus the same decide/decision-class gap on Q27.)

## 3. Options tested on the 40 fixtures (FTS-alone, k<=5)

| config | recall@1 | recall@5 | no-match | adversarial junk |
|--------|----------|----------|----------|------------------|
| porter unicode61 (current) | 33/37 | 35/37 | Q27, Q28 | 0 docs |
| unicode61 (drop porter) | 32/37 | 34/37 | Q8, Q27, Q28 | 0 docs |
| trigram (case_sensitive 0) | 31/37 | 33/37 | Q8, Q27, Q28, Q36 | Q39 matches 6 docs |
| porter + query-side 4-char prefix arm (hybrid) | 34/37 | 37/37 | none | 0 docs |

Dropping porter loses more than it gains: it breaks Q8 (`replies` vs a doc
saying `reply`, plus `tests`/`test`-class pairs) and Q36 slips, for zero
Q28/Q27 gain — the exact-term mismatch (auditors vs audit) is a stem GROUP
mismatch, and plain unicode61 makes every -s/-ing/-ed variant worse.

Trigram is the wrong tool here: Q28 still misses (trigram `"auditors"`
matches 0 docs too — trigrams are character n-grams, not stems; the arm
that would help, `"audi"*`, works identically on the porter index), it
loses Q8/Q36, it makes terms shorter than 3 chars unmatchable (`db`, `v3`,
`ip`, `q1` are in the corpus), it matches junk on Q39 (6 docs), and its
index is ~1.8x the size at 40 docs (750 vs 303 terms; 45 KB vs 24.5 KB).

A "longer min-length porter" does not exist: FTS5 rejects
`porter min_length 4` and `unicode61 min_length 4` ("error in tokenizer
constructor"; min_length is an FTS3/4 option). Tokenizer-level hybrid is
also a dead end: `tokenize='porter trigram'` parses but indexes stemmed
text into trigrams (worse on every axis, `"replies"` matches 0 docs).

The hybrid that works is query-side, not tokenizer-side: `fts_quote`
already quotes each content word; additionally OR in a 4-char prefix
query for words of length >= 5 (`"auditors" OR "audi"*`). Gate at 5:
ungated prefixes let the adversarial queries match 9 junk docs
(Q39 `"how does this work with that"` -> 6 docs); gated at >= 5 they stay
at 0. Gate at 6 also scores 34/37 @1, 37/37 @5 on this corpus; 5 keeps
`check`/`audit`-class 5-letter stems.

4-char prefixes collide in this corpus (work* = worker/workflow, revi* =
review/revision, post* = postgres/postmortems, ...), but bm25 treats the
arm as one low-IDF clause among the OR group; the measured effect is +1
recall@1 (Q29) and 0 regressions on the 40 fixtures.

## 4. Fused-level prediction (real vector lists re-fused)

| config, floor 0.017 | fused@1 | fused@5 | Q27 | Q28 | adversarial |
|---------------------|---------|---------|-----|-----|-------------|
| porter (current, reproduced baseline) | 35/37 | 35/37 | floored (rank 5) | floored (rank 4) | 3/3 |
| porter + 4-char prefix arm | 35/37 | 37/37 | rank 3, score 0.0447 | rank 2, score 0.0455 | 3/3 |

## 5. Recommendation: keep porter, hybrid query arm

Keep `tokenize='porter unicode61'`. Change `fts_quote` (query text only —
no schema, no index rebuild, no trigram table) to emit, per content word
of length >= 5, `"word" OR "word{4prefix}"*`.

Predicted Q28 effect: the `audi*` arm matches the indexed `audit` stem,
FTS ranks the target row, fusion moves Q28 from floored (0.0161 < 0.017)
to rank 2 at ~0.045 — comfortably above the floor, above even 0.02.
Q27 gets the same treatment for free (`deci*` spans decid/decis): rank 3
at ~0.045. Recall@5 goes 35/37 -> 37/37 with recall@1 held at 35/37 and
the adversarial channel unchanged; the two floor-dropped targets were the
entire remaining @5 deficit, and both are stem-gap anchors, so the fix
closes the #6 lexical root cause rather than re-tuning the floor.

Skipped: trigram sidecar index (no gain porter+prefix does not already
give), prefix= index option (tested; +67% index size, no measurable speed
difference at 40 docs — add it if prefix arms ever show up in a latency
profile at corpus scale), custom Rust stemmer (a dictionary stemmer would
be the only way to link auditors/audit exactly, and the 1-line query arm
gets the same fixture outcomes).
