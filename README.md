# ReMem: Unified Modular Memory Mesh

Welcome to the unified **ReMem** engine—a powerful, domain-aware long-term memory system for AI agents.

## 🚀 One Engine, Many Domains

Previously split into RPG and Coding projects, **ReMem** is now a single, high-performance engine that uses a **Modular Mesh Architecture**. It allows your AI to handle complex world-building and expert software engineering in one session without overloading its context window.

### Key Features
- **🧠 Unified Intelligence**: Single core for Graph, Vector, and SQL storage.
- **🧩 Modular Mesh**: On-demand loading of domain-specific tools (RPG, Coding, etc.).
- **📖 Librarian Layer**: Automatic module suggestions based on your conversation context.
- **⚡ Local-Model Optimized**: Designed to run efficiently with local LLMs (13b, 20b+) by preventing tool bloat.

## 🛠️ Getting Started

### Installation
```bash
npm install
npm run build
```

### Usage
Start the MCP server:
```bash
npm start
```

### Managing Modules
Use the built-in Librarian tools:
- `list_modules`: See status of available expertise.
- `activate_module`: Load "RPG" or "Coding" tools on the fly.
- `librarian_suggest`: Let the engine decide what it needs based on your prompt.

## 📂 Project Structure
- `src/core`: Core graph and schema logic.
- `src/infrastructure`: Database (SQLite) and Vector (LanceDB) management.
- `src/modules`: Domain-specific experts (RPG, Coding).
- `src/tests`: Comprehensive verification suite.

## 📜 License
MIT
