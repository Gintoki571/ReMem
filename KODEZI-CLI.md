# ReMem Engine - Coding Agent Guide

## Build & Run Commands
- **Build**: `npm run build` (compiles TypeScript + resolves path aliases)
- **Watch**: `npm run watch` (auto-rebuild on changes)
- **Start Dev**: `npm run start` (runs with ts-node)
- **Start Prod**: `npm run start:prod` (runs compiled dist/index.js)
- **Run Single Test**: `npm run start -- src/tests/test_<name>.ts` or `ts-node --esm src/tests/test_<name>.ts`

## Code Style & Conventions

### Imports
- Always use path aliases: `@core/*`, `@infrastructure/*`, `@application/*`, `@integration/*`, `@shared/*`, `@modules/*`, `@config/*`
- Always append `.js` extension: `from '@core/index.js'`
- Use `import type` for type-only imports: `import type { Node, Edge }`

### Naming Conventions
- **Classes**: PascalCase (`GraphManager`, `BaseToolHandler`)
- **Interfaces**: PascalCase with `I` prefix (`IStorage`, `IManager`)
- **Methods/Variables**: camelCase (`loadGraph`, `graphManager`)
- **Constants**: UPPER_SNAKE_CASE (`MEMORY_FILE_PATH`)
- **Private/Protected**: Use access modifiers, not underscore prefix

### Types & Error Handling
- Use TypeScript strict mode (already enabled)
- Prefer `type` over `interface` for unions/aliases
- Use type guards: `.filter((edge): edge is Edge => edge !== undefined)`
- **Always use error classes from `@shared/errors/index.js`**: `throw new ValidationError()`, `throw new LLMError()`, etc.
- **Always validate inputs with Zod schemas from `@shared/validation/schemas.ts`**: `validateInput(NodeSchema, data)`
- Log errors with context: `console.error('[Component] Error in ${operation}:', error)`

### Configuration
- **Never hardcode magic numbers** - use `CONFIG` from `@config/config.js`
- Examples: `CONFIG.SEARCH.RRF_CONSTANT`, `CONFIG.CONTEXT.L3_MESSAGE_LIMIT`, `CONFIG.EMBEDDINGS.FALLBACK_DIMENSIONS`
- Add new constants to `config.ts` in appropriate sections

### Resource Management
- **Always implement cleanup** for event listeners: store references and remove in `cleanup()` method
- Use atomic operations for file writes (temp file + rename pattern)
- Escape SQL strings: `nodeName.replace(/'/g, "''")`

### Architecture
- Follow layered architecture: `core` → `infrastructure` → `application` → `integration`
- Use abstract base classes for extensibility (`BaseToolHandler`, `IManager`)
- Keep domain logic in `@core`, storage in `@infrastructure`, orchestration in `@application`
- Emit events for cross-cutting concerns (sync, logging, telemetry)

## Recent Improvements (2024)

### Security
- SQL injection prevention in VectorManager (escape quotes)
- Input validation with Zod schemas
- Structured error handling with error codes

### Data Integrity
- Atomic write-rename pattern for file storage
- Event listener cleanup to prevent memory leaks
- Graceful shutdown on SIGINT/SIGTERM

### Features
- Context compaction (L3 → L2 summarization) fully implemented
- Centralized configuration in `config.ts`
- Error hierarchy with actionable suggestions

## Key Files to Know

- **Config**: `src/config/config.ts` - All tunable parameters
- **Validation**: `src/shared/validation/schemas.ts` - Zod schemas for inputs
- **Errors**: `src/shared/errors/index.ts` - Error classes and utilities
- **Analysis**: `COMPREHENSIVE_ANALYSIS.md` - Full codebase analysis
- **Fixes**: `FIXES_APPLIED.md` - Detailed changelog
- **Quick Ref**: `QUICK_REFERENCE.md` - Quick reference guide
