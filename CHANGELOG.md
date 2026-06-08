# Changelog

## v0.2.0 (2026-06-08)

### 🚀 New Features

- **hyper swarm** — Parallel multi-agent execution for complex tasks
- **hyper analyze** — Deep codebase analysis (compile, security, dead code, complexity)
- **hyper ci-fix** — Auto-fix CI failures from logs
- **hyper eval** — Self-evaluation benchmark suite
- **hyper feedback** — Agent self-correction (RLHF-lite)
- **hyper skill** — Community skill marketplace (install/search/create)
- **hyper session share/join** — Session sharing via tokens
- **hyper memory global** — Cross-project global knowledge layer
- **SSE streaming** — Token-by-token output for Web UI
- **PR Review Bot** — Automatic PR code reviews
- **VS Code Extension v0.2** — Chat panel + server integration
- **i18n zh-CN** — Full Chinese CLI experience
- **Homebrew formula** — `brew install` support
- **cargo-binstall** — Fast binary install

### ⚡ Performance

- BM25 batch-load: N+1 → 2 SQL queries (14-40x query speedup)
- r2d2 connection pool (4 connections) + WAL mode
- OnceLock entity regex (8 compilations → 1)
- Insert transaction batching
- FTS5 full-text search engine
- n-gram hash embedding (256 dimensions)

### 🧠 Memory

- 14 memory types with auto-classification
- Auto-prune with configurable thresholds
- forget_below() and forget_older_than() bulk operations
- BM25 + entity + temporal + importance + vector 5-signal fusion
- Global cross-project knowledge promotion

### 🏗️ Architecture

- axum HTTP server with SSE streaming
- Docker sandbox for tool execution
- ProviderPool with health checks + circuit breaker
- MCP client/server (JSON-RPC 2.0)
- Plugin system with hooks (10 events)
- Skill marketplace (YAML frontmatter format)

### 📦 Distribution

- `brew install nick-zhang-1991/hyperagent/hyperagent`
- `cargo binstall hyperagent`
- `cargo install hyperagent`
- `curl | bash` install script
- PowerShell install script for Windows

### 🧪 Quality

- 24 tests (memory 21 + retrieval 3)
- 0 lint warnings
- Self-evaluation benchmark framework

---

## v0.1.0 (2026-05)

- Initial release
- Multi-agent pipeline (Plan → Code → Review → Apply)
- Tree-sitter code index with PageRank
- Basic memory system
- Git worktree isolation
- CLI: run, review, init, session
