# HyperAgent Architecture

## System Overview

```mermaid
graph TB
    subgraph "User Interfaces"
        CLI[CLI Terminal]
        WEB[Web UI - Vite+React]
        DESK[Desktop - Tauri v2]
        VSCODE[VS Code Extension]
    end

    subgraph "HTTP Server"
        SERVE[hyper serve]
        SSE[SSE Streaming]
        API[API Routes]
    end

    subgraph "Agent Pipeline"
        PLAN[Plan Agent]
        CODE[Code Agent]
        REVIEW[Review Agent]
        APPLY[Apply Agent]
        FIX[Auto-Fix Loop]
    end

    subgraph "Memory System"
        MEM[MemoryManager]
        FTS[FTS5 Search]
        EMB[n-gram Embedding]
        GLOBAL[Global Memory]
        PRUNE[Auto-Prune]
    end

    subgraph "Tools"
        MCP[MCP Client/Server]
        WEBSEARCH[Web Search]
        BROWSER[Browser Automation]
        DESKTOP[Desktop Automation]
        SANDBOX[Docker Sandbox]
    end

    subgraph "Infrastructure"
        INDEX[Tree-sitter + PageRank]
        PROVIDER[ProviderPool + Failover]
        DB[(SQLite + r2d2 + WAL)]
        PLUGIN[Plugin System]
        SKILL[Skill Marketplace]
    end

    CLI --> SERVE
    WEB --> SERVE
    DESK --> SERVE
    VSCODE --> SERVE
    
    SERVE --> PLAN
    PLAN --> CODE
    CODE --> REVIEW
    REVIEW --> APPLY
    APPLY --> FIX
    
    PLAN --> MEM
    CODE --> MEM
    MEM --> GLOBAL
    MEM --> PRUNE
    
    PLAN --> MCP
    PLAN --> WEBSEARCH
    PLAN --> BROWSER
    
    PLAN --> PROVIDER
    INDEX --> PROVIDER
    DB --> MEM
    SKILL --> PLAN
```

## Data Flow

```mermaid
sequenceDiagram
    participant U as User
    participant S as hyper serve
    participant O as Orchestrator
    participant M as MemoryManager
    participant P as LLM Provider
    participant T as Tools

    U->>S: POST /api/chat (SSE)
    S->>O: Run task
    O->>M: recall_fused(query)
    M-->>O: BM25+entity+temporal scores
    O->>M: global_search(query)
    M-->>O: cross-project knowledge
    O->>P: chat_stream(messages)
    P-->>S: SSE tokens
    S-->>U: data: {token}\n\n
    O->>T: execute tool calls
    T-->>O: tool results
    O->>M: remember(result)
    O->>S: [DONE]
```

## Module Map

```
src/
├── cli/           CLI commands (init, run, serve, analyze, swarm...)
├── agent/         Agent pipeline
│   ├── orchestrator.rs   Master agent loop
│   ├── mediator.rs       Agent coordination
│   └── tools.rs          Tool dispatch
├── llm/           LLM providers
│   ├── provider.rs       OpenAl-compatible API
│   ├── pool.rs           Provider failover pool
│   └── streaming.rs      SSE streaming response
├── memory.rs      Memory system (14 modes, FTS5, embedding)
├── index/         Tree-sitter code index + PageRank
├── serve.rs       HTTP server (axum + SSE)
├── analyze.rs     Deep code analysis
├── swarm.rs       Multi-agent parallel execution
├── ci_fix.rs      CI auto-fix
├── skill_market.rs Skill marketplace
├── eval.rs        Self-evaluation benchmark
├── sandbox.rs     Docker sandbox
├── mcp.rs         MCP client/server
├── plugin.rs      Plugin system
├── session.rs     Session management + share
├── i18n.rs        Internationalization (en + zh-CN)
└── config.rs      Configuration management
```
