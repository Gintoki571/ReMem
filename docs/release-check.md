# Release parity check

- git rev: a7b522ea3185786d92d17d6b57f8eb3ec33649ef (branch v3)
- `target/release/remem`: 13395184 bytes (12.8 MB)
- `target/release/remem-mcp`: 12817392 bytes (12.2 MB)
- MCP tool count: 7 (remember, recall, list, link, forget, stats, validate)
- remember+recall roundtrip on release binary (scratch db /tmp/remem-release-check.db): Y
- vs docs/perf.md: sizes within ~0.5 MB of recorded 12.8/12.2 MB; no regression.
