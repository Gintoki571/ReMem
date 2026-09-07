# ReMem v2

Local-first long-term memory engine for AI agents. TypeScript, Node 22+.

## Architecture

- **Storage**: SQLite via Node built-in `node:sqlite` (WAL, FTS5). Zero native deps, single file.
- **Vectors**: local ONNX embedding model (cadet-embed-base-v1, 768-dim), CUDA execution provider with CPU fallback.
- **Graph/events**: LatticeDB integration (in progress).

## Roadmap

- [x] Project scaffold, domain types
- [x] SQLite + FTS5 store (TDD)
- [ ] Embedding service (GPU + CPU fallback)
- [ ] Vector recall (KNN + score fusion with FTS)
- [ ] LatticeDB graph/event layer
- [ ] MCP server + prime-agent skill
- [ ] Dogfood from live agent sessions

## Development

```bash
npm install
npm run typecheck
npm test
npm run lint
```
