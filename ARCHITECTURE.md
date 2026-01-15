# ReMem_Engine - System Architecture (Post-Fixes)

## High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         MCP Server (stdio)                       │
│                         src/index.ts                             │
└────────────────────────┬────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Tools Registry (Singleton)                    │
│              src/integration/tools/registry/                     │
│  ┌──────────────────┐  ┌──────────────────┐  ┌───────────────┐ │
│  │  Static Tools    │  │  Dynamic Tools   │  │ Module Tools  │ │
│  │  (Core + Auto)   │  │  (Schema-based)  │  │ (RPG/Coding)  │ │
│  └──────────────────┘  └──────────────────┘  └───────────────┘ │
└────────────────────────┬────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────────┐
│                   Tool Handler Factory                           │
│              src/integration/tools/handlers/                     │
│  Routes tool calls to appropriate handlers                       │
└────────────────────────┬────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────────┐
│                   Application Manager (Facade)                   │
│              src/application/managers/                           │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐  │
│  │ Graph Mgr    │  │ Search Mgr   │  │ Transaction Mgr      │  │
│  │ Context Mgr  │  │ Sync Service │  │ (with cleanup!)      │  │
│  └──────────────┘  └──────────────┘  └──────────────────────┘  │
└────────────────────────┬────────────────────────────────────────┘
                         │
         ┌───────────────┼───────────────┐
         ▼               ▼               ▼
┌─────────────┐  ┌─────────────┐  ┌─────────────┐
│   Core      │  │Infrastructure│  │ Integration │
│   Domain    │  │   Storage    │  │   Tools     │
└─────────────┘  └─────────────┘  └─────────────┘
```

---

## Data Flow (Auto Add Memory)

```
User Input: "The dragon Smaug guards treasure"
         │
         ▼
┌─────────────────────────────────────────────────────────────────┐
│ 1. MCP Server receives CallToolRequest                          │
│    Tool: auto_add_memory                                         │
│    Args: { text: "..." }                                         │
└────────────────────────┬────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────────┐
│ 2. Tools Registry validates and routes                          │
│    ✅ NEW: Input validation (Zod schema)                        │
└────────────────────────┬────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────────┐
│ 3. AutoMemoryToolHandler processes                              │
│    - Calls Analyzer.extractFromText()                           │
│    - ✅ NEW: Better error logging                               │
└────────────────────────┬────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────────┐
│ 4. Analyzer extracts entities via LLM                           │
│    Result: { entities: [Smaug, treasure], relationships: [...] }│
│    ✅ NEW: Structured error handling                            │
└────────────────────────┬────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────────┐
│ 5. ApplicationManager.addNodes()                                │
│    - GraphManager → GraphOperations → NodeManager               │
│    - Emits 'afterAddNodes' event                                │
└────────────────────────┬────────────────────────────────────────┘
                         │
         ┌───────────────┼───────────────┐
         ▼               ▼               ▼
┌─────────────┐  ┌─────────────┐  ┌─────────────┐
│ JSON Lines  │  │   SQLite    │  │  LanceDB    │
│ (Primary)   │  │ (Secondary) │  │  (Vectors)  │
│             │  │             │  │             │
│ ✅ Atomic   │  │ ✅ Synced   │  │ ✅ Escaped  │
│    writes   │  │    via      │  │    queries  │
│             │  │    events   │  │             │
└─────────────┘  └─────────────┘  └─────────────┘
```

---

## Storage Architecture (Triple Store)

```
┌─────────────────────────────────────────────────────────────────┐
│                        Primary Storage                           │
│                    data/memory.json                              │
│                                                                   │
│  Format: JSON Lines (one object per line)                        │
│  Content: Nodes + Edges                                          │
│  ✅ NEW: Atomic write-rename pattern                            │
│  ✅ NEW: Serialized writes (no race conditions)                 │
│                                                                   │
│  {"type":"node","name":"Smaug","nodeType":"npc",...}            │
│  {"type":"edge","from":"Smaug","to":"treasure",...}             │
└─────────────────────────────────────────────────────────────────┘
                         │
                         │ Synced via Events
                         ▼
┌─────────────────────────────────────────────────────────────────┐
│                      Secondary Storage                           │
│                     data/remem.db (SQLite)                       │
│                                                                   │
│  Tables: nodes, edges, embeddings, messages, global_state        │
│  Purpose: Fast queries, relationships, context                   │
│  ✅ NEW: Event-driven sync with cleanup                         │
│                                                                   │
│  SELECT * FROM nodes WHERE nodeType = 'npc'                      │
│  SELECT * FROM edges WHERE fromNode = 'Smaug'                    │
└─────────────────────────────────────────────────────────────────┘
                         │
                         │ Parallel Storage
                         ▼
┌─────────────────────────────────────────────────────────────────┐
│                       Vector Storage                             │
│                   data/lancedb/ (LanceDB)                        │
│                                                                   │
│  Table: memory_vectors                                           │
│  Purpose: Semantic search                                        │
│  ✅ NEW: SQL injection prevention (escaped queries)             │
│                                                                   │
│  {id, text, vector[384], nodeName, nodeType, metadata}          │
└─────────────────────────────────────────────────────────────────┘
```

---

## Context Management (3-Tier System)

```
┌─────────────────────────────────────────────────────────────────┐
│                    L1: System Context                            │
│                    (Global Facts)                                │
│                                                                   │
│  - System profile                                                │
│  - Global facts (scope: system)                                  │
│  - Always included in prompts                                    │
│                                                                   │
│  Source: nodes with nodeType='global_fact'                       │
└─────────────────────────────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────────┐
│                    L2: Summary Context                           │
│                    (Rolling Summary)                             │
│                                                                   │
│  - Compressed history of past conversations                      │
│  - Updated via LLM summarization                                 │
│  - ✅ NEW: Fully implemented compactContext()                   │
│                                                                   │
│  Source: node with name='{userId}_summary'                       │
│  Trigger: When unsummarized messages > threshold                 │
└─────────────────────────────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────────┐
│                    L3: Conversation Context                      │
│                    (Recent Messages)                             │
│                                                                   │
│  - Last N messages (default: 10)                                 │
│  - Full detail, not compressed                                   │
│  - ✅ NEW: Configurable via CONFIG.CONTEXT.L3_MESSAGE_LIMIT     │
│                                                                   │
│  Source: messages table (ORDER BY createdAt DESC LIMIT N)        │
└─────────────────────────────────────────────────────────────────┘
```

---

## Error Handling Flow

```
┌─────────────────────────────────────────────────────────────────┐
│                    Operation Attempted                           │
│              (e.g., addNodes, generateEmbedding)                 │
└────────────────────────┬────────────────────────────────────────┘
                         │
                         ▼
                    ┌─────────┐
                    │ Success?│
                    └────┬────┘
                         │
         ┌───────────────┼───────────────┐
         │ YES           │ NO            │
         ▼               ▼               ▼
┌─────────────┐  ┌─────────────┐  ┌─────────────┐
│   Return    │  │   Catch     │  │  Validate   │
│   Result    │  │   Error     │  │   Input     │
└─────────────┘  └──────┬──────┘  └──────┬──────┘
                        │                 │
                        ▼                 ▼
              ┌──────────────────┐  ┌──────────────────┐
              │ Is RememError?   │  │ ValidationError? │
              └────┬─────────────┘  └────┬─────────────┘
                   │                      │
         ┌─────────┼─────────┐           │
         │ YES     │ NO      │           │
         ▼         ▼         ▼           ▼
┌─────────────┐ ┌─────────────┐ ┌─────────────────────┐
│ Structured  │ │ Wrap in     │ │ Return with field   │
│ Error with  │ │ RememError  │ │ and suggestion      │
│ - code      │ │             │ │                     │
│ - details   │ │             │ │ ✅ NEW: Zod errors  │
│ - suggestion│ │             │ │     with context    │
└──────┬──────┘ └──────┬──────┘ └──────┬──────────────┘
       │               │               │
       └───────────────┼───────────────┘
                       │
                       ▼
              ┌─────────────────┐
              │ Log with context│
              │ Return to client│
              └─────────────────┘
```

---

## Module System (Dynamic Tool Loading)

```
┌─────────────────────────────────────────────────────────────────┐
│                    Server Initialization                         │
└────────────────────────┬────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────────┐
│              CONFIG.MODULES.ACTIVE = ['rpg', 'coding']           │
│              (from env or default)                               │
└────────────────────────┬────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────────┐
│                   SchemaLoader.loadAllSchemas()                  │
│  - Loads core schemas                                            │
│  - Loads schemas from active modules                             │
│  - Filters by CONFIG.MODULES.ACTIVE                              │
└────────────────────────┬────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────────┐
│              DynamicSchemaToolRegistry.initialize()              │
│  - Generates add/update/delete tools per schema                  │
│  - Registers in toolsRegistry                                    │
└────────────────────────┬────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────────┐
│                    MCP ListToolsRequest                          │
│  Returns: Core tools + Active module tools                       │
│                                                                   │
│  Core: auto_add_memory, search_nodes, query_sql_db, ...         │
│  RPG: add_npc, add_quest, add_location, ...                      │
│  Coding: add_bug, add_task, add_code_file, ...                   │
└─────────────────────────────────────────────────────────────────┘
```

---

## Configuration Hierarchy

```
┌─────────────────────────────────────────────────────────────────┐
│                    Environment Variables                         │
│  OPENAI_API_KEY, OPENAI_BASE_URL, LLM_MODEL, etc.               │
└────────────────────────┬────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────────┐
│                    src/config/config.ts                          │
│  ✅ NEW: Centralized configuration                              │
│                                                                   │
│  SERVER:     { NAME, VERSION }                                   │
│  PATHS:      { DATA_DIR, MEMORY_FILE, ... }                      │
│  SEARCH:     { RRF_CONSTANT, DEFAULT_LIMIT, ... }                │
│  CONTEXT:    { L3_MESSAGE_LIMIT, COMPACTION_THRESHOLD }          │
│  EMBEDDINGS: { FALLBACK_DIMENSIONS, DEFAULT_MODEL }              │
│  LLM:        { DEFAULT_MODEL, MAX_RETRIES, TIMEOUT_MS }          │
│  VALIDATION: { MAX_NODE_NAME_LENGTH, MAX_TEXT_LENGTH }           │
└────────────────────────┬────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Used Throughout Codebase                      │
│  import { CONFIG } from '@config/config.js'                      │
│  const limit = CONFIG.SEARCH.DEFAULT_LIMIT                       │
└─────────────────────────────────────────────────────────────────┘
```

---

## Security Layers

```
┌─────────────────────────────────────────────────────────────────┐
│                    Input Layer                                   │
│  ✅ NEW: Zod validation schemas                                 │
│  ✅ NEW: Type-safe input parsing                                │
└────────────────────────┬────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Processing Layer                              │
│  ✅ NEW: SQL injection prevention (escaped queries)             │
│  ✅ NEW: Atomic file operations (no corruption)                 │
└────────────────────────┬────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Storage Layer                                 │
│  ✅ NEW: Write locks (no race conditions)                       │
│  ✅ NEW: Temp file + rename (atomic writes)                     │
└────────────────────────┬────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Error Layer                                   │
│  ✅ NEW: Structured errors (no info leakage)                    │
│  ✅ NEW: Error codes (no stack traces to client)                │
└─────────────────────────────────────────────────────────────────┘
```

---

## Lifecycle Management

```
┌─────────────────────────────────────────────────────────────────┐
│                    Server Start                                  │
│  1. Load config                                                  │
│  2. Initialize ApplicationManager                                │
│  3. Initialize toolsRegistry                                     │
│  4. Register MCP handlers                                        │
│  5. Connect stdio transport                                      │
└────────────────────────┬────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Runtime                                       │
│  - Handle tool calls                                             │
│  - Sync to secondary stores                                      │
│  - ✅ NEW: Event listeners tracked                              │
└────────────────────────┬────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Shutdown (SIGINT/SIGTERM)                     │
│  1. ✅ NEW: knowledgeGraphManager.cleanup()                     │
│  2. ✅ NEW: Remove all event listeners                          │
│  3. Close MCP server                                             │
│  4. Exit gracefully                                              │
└─────────────────────────────────────────────────────────────────┘
```

---

## Key Improvements Summary

### 🔒 Security
- SQL injection prevention in VectorManager
- Input validation with Zod schemas
- Structured error handling (no info leakage)

### 🛡️ Data Integrity
- Atomic write-rename pattern
- Serialized writes (no race conditions)
- Event listener cleanup (no memory leaks)

### ✨ Features
- Context compaction fully implemented
- Centralized configuration
- Error hierarchy with suggestions

### 📊 Observability
- Better error logging with context
- Structured error codes
- Cleanup tracking

---

**Architecture Status**: ✅ Production-Ready (Single-User)  
**Last Updated**: January 15, 2025  
**Version**: 0.3.0 (Post-Analysis)
