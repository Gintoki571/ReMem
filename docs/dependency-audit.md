# Dependency audit — v3 workspace (2026-09-09)

Method: `cargo tree --duplicates` (workspace) + OSV API (`https://api.osv.dev/v1/query`,
ecosystem `crates.io`, exact pinned versions). `cargo audit` NOT installed; install
skipped (source build exceeds the 5-min budget). OSV aggregates RustSec, so coverage
is equivalent for known CVEs. No `tokio` in tree (sync CLI/MCP).

## Duplicates: 7 crate names, 2 actionable

| Crate | Versions | Source | Action |
|---|---|---|---|
| tokenizers | 0.22.2 / 0.23.2 | 0.23 direct (`remem-embed`); 0.22 via `candle-* 0.11` | Accept until candle upgrades; two tokenizers add build weight |
| zerocopy | 0.7.35 / 0.8.56 | 0.7 pinned in `remem-store`; 0.8 via ecosystem | Drop the `zerocopy = "0.7"` pin if 0.8 API suffices |
| getrandom | 0.3.4 / 0.4.3 | Knock-on (ahash/rand vs tempfile/uuid) | None — transitive only |
| hashbrown | 0.14.5 / 0.16.1 / 0.17.1 | Knock-on (hashlink vs safetensors vs zip) | None — transitive only |
| syn | 2.0.119 / 3.0.5 | Knock-on (derive macros) | None — transitive only |
| thiserror (+ -impl) | 1.0.69 / 2.0.20 | Knock-on (tokenizers 0.22 uses v1, 0.23 uses v2) | Resolves itself if tokenizers unifies |
| — | — | — | — |

## CVE verdict: CLEAN (17 versioned queries, 0 vulns)

Checked via OSV API, all zero findings: rusqlite 0.32.1, serde 1.0.229,
serde_json 1.0.151, clap 4.6.6, anyhow 1.0.104, candle-core 0.11.0,
tokenizers 0.22.2 + 0.23.2, sqlite-vec 0.1.9, zerocopy 0.7.35 + 0.8.56,
syn 2.0.119, thiserror 2.0.20, getrandom 0.3.4, hashbrown 0.14.5,
uuid 1.26.0, sha2 0.10.9. (Old rusqlite RUSTSEC-2020-0014 predates 0.23; not applicable.)

## Update policy (suggestion)

- Monthly: `cargo update` on a scratch checkout, run tests, commit `Cargo.lock` if green.
- Security: subscribe to RustSec advisories for rusqlite / sqlite-vec / candle
  (unsafe + FFI surface); patch out-of-band on any alert.
- Watcher: whoever owns the release checklist runs the monthly bump.
- Revisit the tokenizers/zerocopy splits when `candle 0.12+` lands.
