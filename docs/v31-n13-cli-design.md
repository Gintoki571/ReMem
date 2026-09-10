# v3.1 Lane N13 — recall CLI/env wiring (issue #13) — PHASE 1 DESIGN

## Status: DONE — implemented, suite green (lane-n13, post lane-c merge)

Phase 2 built exactly this spec: `git merge lane-c`; `main.rs` wiring;
`tests/lane_n13.rs` 6/6 green; full `cargo test -p remem-recall` 96 passed
(30 lib + 9 main + 11 cli + 29 engine + 11 lane_c + 6 lane_n13), 0 failed;
`cargo fmt --check` clean. Original Phase-1 design below, kept as-is.

Rank API lives on lane-c (still in verification). This doc is the full
Phase-2 build spec. Canonical coordination copy: `/home/bindesh/rag/reports/v31-n13.md`
(branch `lane-n13` carries a byte-identical copy at `docs/v31-n13-cli-design.md`).

## Scope (from verify-v31-c.md §5)

Lane C delivered engine builder API only. `crates/remem-recall/src/main.rs`
has zero recency/half-life flags (grep-clean), and `docs/temporal-eval.md`
still states "No `--no-recency` flag exists". This lane wires the builders
to CLI flags + env. No ranking behavior changes; `scripts/eval.sh` must
reproduce the lane-C numbers (recall@1 35/37, recall@5 37/37, adversarial 3/3).

## Rank API target (from v31-c.md, post-merge)

- `with_recency_off()` — skips recency band + `recent` reasons.
- `with_half_life_days(days)` — recency half-life, default `DEFAULT_HALF_LIFE_DAYS = 30.0`.
- `DEFAULT_COSINE_FLOOR = 0.63` — engine-internal abstention floor, **no CLI
  override in this issue** (stays hard-wired in `recall()`).
- Signature note: lane-c moved builders to `&self` (Arc-shared engine);
  base here is `mut self`. The wiring below rebinds (`eng = eng.with_...()`),
  which compiles under either shape — confirm with the compiler after merge.

## Flags (all on `Cmd::Recall` only)

| Flag | Type | Default | Env | Wires to |
|---|---|---|---|---|
| `--no-recency` | bool (SetTrue) | false | `REMEM_NO_RECENCY` | `with_recency_off()` iff set |
| `--half-life-days <DAYS>` | f64 | `remem_recall::DEFAULT_HALF_LIFE_DAYS` | `REMEM_HALF_LIFE_DAYS` | `with_half_life_days(days)` |
| `--min-score <F>` | f64 (existing, unchanged name/default `DEFAULT_MIN_SCORE`) | 0.0 = off | `REMEM_COSINE_FLOOR` (new) | `with_min_score(f)` (unchanged) |

Notes:

- `REMEM_COSINE_FLOOR` feeds the **score floor** (`with_min_score`), not the
  cosine abstention floor — env name is mandated by issue #13, kept verbatim.
- Precedence is clap-native: CLI flag > env > default. No custom parsing.
- Bool env: clap parses `true`/`false` (case-insensitive) for `REMEM_NO_RECENCY`.
- Validation (one guard, Phase 2): `days <= 0.0 || !days.is_finite()` ->
  `anyhow!("invalid --half-life-days '{days}' (must be finite and > 0)")`.
  `--min-score` keeps its current no-validation behavior (0 = off).

## Arg parsing shape (`crates/remem-recall/src/main.rs`)

```rust
/// Disable the recency band (score contribution + `recent` reasons).
#[arg(long, env = "REMEM_NO_RECENCY")]
no_recency: bool,
/// Recency half-life in days.
#[arg(long, env = "REMEM_HALF_LIFE_DAYS",
      default_value_t = remem_recall::DEFAULT_HALF_LIFE_DAYS)]
half_life_days: f64,
// existing field, add env only:
#[arg(long, env = "REMEM_COSINE_FLOOR",
      default_value_t = remem_recall::DEFAULT_MIN_SCORE)]
min_score: f64,
```

Match arm (rebind form — compiles under `mut self` or `&self` builders):

```rust
Cmd::Recall { query, k, json, agent, session, since, until,
              max_chars, min_score, no_recency, half_life_days } => {
    if !half_life_days.is_finite() || half_life_days <= 0.0 {
        return Err(anyhow!(
            "invalid --half-life-days '{half_life_days}' (must be finite and > 0)"));
    }
    // ... RecallQuery build unchanged ...
    let eng = engine(&cli.db)?.with_min_score(min_score)
        .with_half_life_days(half_life_days);
    let eng = if no_recency { eng.with_recency_off() } else { eng };
    let hits = eng.recall(&q)?;
    // ... output unchanged ...
}
```

## Docs touched in Phase 2

- `docs/temporal-eval.md`: delete/replace the "No `--no-recency` flag exists" line.
- `AGENTS.md` CLI line: append `[--no-recency] [--half-life-days d]` to the
  `recall` synopsis (one-line DOX pass, no contract change).

## Phase-2 test plan (TDD, RED first)

New CLI integration tests (`crates/remem-recall/tests/cli*.rs` or lane file):

1. `--help` for `recall` lists all three flags.
2. `--no-recency` drops every `recent` reason vs same query without it.
3. Each env var alone reproduces its flag (`REMEM_NO_RECENCY=true`,
   `REMEM_HALF_LIFE_DAYS=7`, `REMEM_COSINE_FLOOR=0.99` narrows vs default).
4. CLI flag beats env (`REMEM_HALF_LIFE_DAYS=7` + `--half-life-days 30`
   == plain `--half-life-days 30` output).
5. `--half-life-days 0` / `-3` / `nan` exits non-zero with `half-life` on stderr.
6. Full `cargo test -p remem-recall` + `cargo fmt --check` green; eval delta none.

## Phase-2 steps

1. `git merge lane-c` into `lane-n13` (resolve builder-shape drift if any).
2. RED tests above, implement wiring per this doc, suite + fmt green.
3. Update the two docs lines. Commit on `lane-n13`. Never push; never touch
   other lane branches.
