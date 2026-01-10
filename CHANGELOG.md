## Changelog

### v0.3.0 (2025-01-10)

#### 🚀 Major Architectural Refactor
- **Unified Tool Dispatch:** Consolidated all tool calls through `ToolsRegistry` and `ToolHandlerFactory`.
- **Infrastructure Synchronization:** Introduced `InfrastructureSyncService` for automatic, event-driven mirroring of graph changes to SQLite and Vector stores.
- **Minimalist Mode:** Expose only high-value tools to the AI to prevent context bloat.
- **Improved Performance:** Optimized WAL mode in SQLite and pre-emptive handler initialization.

#### 📁 Project Structure Cleanup
- Moved `schemas` to `data/schemas` at the project root.
- Cleaned up redundant files and unified configuration paths.


### v0.2.8 (2024-12-24)

#### Features

- **Edge Weights:**
    - Introduced an optional `weight` property to the `Edge` interface to represent relationship strength in the range of 0-1.
    - Edge weights now default to 1 if not specified.

- **Enhanced Search:**
  - Modified `SearchManager` to include immediate neighbor nodes in `searchNodes` and `openNodes` results.

**Impact:**

- **Edge Weights:**
  - Enables a more nuanced representation of relationships in the knowledge graph, allowing for the expression of varying degrees of connection strength or confidence.
  - No changes in schemas.
  - 
- **Enhanced Search:**
  - Provides a more comprehensive view of the relevant portion of the knowledge graph returning more contextually relevant information to the AI.
