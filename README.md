# HyperAgent

**Ultra-fast CLI coding agent** — parallel multi-agent pipeline with global code understanding, automatic failover, and production-ready security.

```bash
# One-shot task
hyper run "add rate limiting to the API gateway"

# Interactive REPL
hyper

# Review changes side-by-side
hyper diff --side-by-side --staged
```

## Quick Start

```bash
# 1. Build
cargo build --release
cp target/release/hyperagent ~/.local/bin/hyper

# 2. Configure (auto-detects ~/Library/Application Support/hyper/config.toml)
hyper config-init

# 3. Run
hyper init                          # Build code index (first time)
hyper run "refactor auth module"    # Run a coding task
```

**No env vars needed** — reads from config file automatically. Override with:
```bash
export HYPER_LLM_API_KEY="sk-..."
export HYPER_LLM_BASE_URL="http://localhost:4006/v1"
hyper run "..." --mode ask
```

## Architecture

```
┌─ User Prompt ─────────────────────────────────────────────┐
│                                                            │
│  ┌─ Ask Mode ──────────────────────────────────────────┐   │
│  │  Conversation History → Memory → PageRank Index      │   │
│  │  → Direct LLM (with MCP tool calling)                │   │
│  │  → Response (streamed to terminal)                   │   │
│  └──────────────────────────────────────────────────────┘   │
│                                                            │
│  ┌─ Code/Architect/Debug Mode ─────────────────────────┐   │
│  │  Conversation History (last 10 turns)                │   │
│  │  → Memory Context (persistent learnings from SQLite) │   │
│  │  → PageRank Index (relevant file ranking)            │   │
│  │  → PlanAgent (task decomposition, N parallel steps)  │   │
│  │  → CodeAgents (N-way parallel tokio::spawn)          │   │
│  │  → Streaming output per agent (real-time tokens)     │   │
│  │  → ReviewAgent (merged spec+quality, skip trivial)   │   │
│  │  → Auto-lint + fix loop (3 rounds cargo check/tsc)   │   │
│  │  → ApplyAgent (surgical diff application)            │   │
│  │  → Memory auto-record + session save                 │   │
│  └──────────────────────────────────────────────────────┘   │
│                                                            │
│  ┌─ Provider Pool (automatic failover) ─────────────────┐  │
│  │  DeepSeek (primary) → OpenAI (backup)                 │   │
│  │  Cooldown tracking: 5s → 50s → 500s exponential      │   │
│  └──────────────────────────────────────────────────────┘   │
└────────────────────────────────────────────────────────────┘
```

## Features

### Core Pipeline
- **Multi-agent parallelism**: Plan → N CodeAgents → Review → Apply
- **Streaming output**: Real-time LLM token display per agent
- **Lint auto-fix**: 3-round cargo check/tsc repair loop
- **Memory persistence**: SQLite-backed entity-importance scoring
- **Session management**: Fork/merge/tree/branch history

### Provider & Reliability
- **Multi-provider failover**: Auto-fallback between DeepSeek, OpenAI, etc.
- **Exponential cooldown**: 5s → 50s → 500s after repeated failures
- **Config hot-reload**: Changes to config.toml picked up automatically
- **Adaptive retry**: 3 attempts with exponential backoff on 5xx/429/timeout

### Code Understanding
- **PageRank file ranking**: Cross-file reference graph with 4-strategy import resolution
- **Incremental index**: File watcher updates per-file, no full rebuild
- **Multi-language parser**: Rust, Python, JavaScript, TypeScript, Go, Java
- **.gitignore-aware scanning**: Uses ignore crate, respects .hyperignore

### Security
- **Dangerous command detection**: rm -rf /, curl|bash, dd, fork bombs blocked
- **Git safety**: Force push, hard reset, clean -fd prevented
- **Hook-level security**: All lifecycle hooks checked before execution
- **Path traversal protection**: File paths validated against project root
- **10MB file size limit**: Prevents runaway writes

### Diff & Review
- **Side-by-side diff**: `hyper diff --side-by-side` for visual comparison
- **Unified diff**: Color-coded additions/deletions/headers
- **Staged/unstaged**: Review both cached and working tree changes
- **AI code review**: `hyper review` with two-stage spec + quality review

### Developer Experience
- **Interactive REPL**: `/mode`, `/reindex`, `/memory`, `/clear` commands
- **Command history**: rustyline-based, persisted across sessions
- **Cost tracking**: Per-run ~$ estimate with budget limits
- **TUI dashboard**: `hyper tui` (feature-gated with ratatui)
- **Session hot resume**: `hyper run --session last`

### Integrations
- **MCP tools**: Connect to Model Context Protocol servers for tool calling
- **Web search**: `hyper search` — DuckDuckGo API, no API key needed
- **Docker deploy**: `hyper deploy --tag myapp:latest`
- **Project scaffolding**: `hyper scaffold myapp --type rust|python|ts`
- **Knowledge base**: `hyper knowledge build/search` — SQLite-backed BM25

## Configuration

### Config File (`~/Library/Application Support/hyper/config.toml`)

```toml
[[providers]]
name = "deepseek"
api_key = "sk-..."
base_url = "https://api.deepseek.com/v1"
default_model = "deepseek-v4-flash"
models = ["deepseek-v4-flash", "deepseek-v4-pro"]
priority = 1
input_price_per_1m = 0.15
output_price_per_1m = 0.60

[[providers]]
name = "openai"
api_key = "sk-..."
base_url = "https://api.openai.com/v1"
default_model = "gpt-4o"
models = ["gpt-4o", "gpt-4o-mini"]
priority = 2
input_price_per_1m = 2.50
output_price_per_1m = 10.00

[[agents]]
name = "build"
model = "deepseek-v4-flash"
mode = "primary"
temperature = 0.1
permissions = { edit = "allow", bash = "allow", read = "allow", network = "deny" }
```

When multiple providers are configured, HyperAgent automatically uses them as a failover pool: primary provider failures trigger automatic fallback with cooldown tracking.

### Env Var Override (no config file needed)

```bash
export HYPER_MODEL="deepseek-v4-flash"
export HYPER_LLM_BASE_URL="http://127.0.0.1:4006/v1"
export HYPER_LLM_API_KEY="sk-..."
```

## Commands

| Command | Description |
|---------|-------------|
| `hyper` | Interactive REPL (ask mode default) |
| `hyper run "<task>"` | Run coding task with agent pipeline |
| `hyper init [--force]` | Build/rebuild PageRank index |
| `hyper commit [-m "msg"]` | Stage all + commit + push |
| `hyper diff [--staged] [--side-by-side] [ref]` | Colorized diff viewer |
| `hyper review [--against main]` | AI code review |
| `hyper search <query>` | Web search (no API key needed) |
| `hyper doctor` | System diagnostics |
| `hyper mode list\|show` | Agent mode management |
| `hyper memory list\|search\|entities` | Persistent memory |
| `hyper session list\|view\|fork\|export` | Session management |
| `hyper deploy --tag <tag>` | Docker build |
| `hyper scaffold <name> --type <type>` | Project scaffolding |
| `hyper knowledge build\|search` | RAG knowledge base |
| `hyper deps` | Dependency graph |
| `hyper hooks list\|fire` | Lifecycle hooks |
| `hyper tui` | Terminal dashboard (feature-gated) |
| `hyper test-gen <file>` | Auto-generate unit tests |

## Test Suite

```bash
cargo test                    # 56+ tests across all modules
cargo test diff_view::tests   # Diff viewer tests
cargo test security::tests    # Security sandbox tests
cargo test llm::pool::tests   # Provider failover tests
```

## Performance

| Mode | Before | After | Savings |
|------|--------|-------|---------|
| Ask mode | 3-16s | 2-10s | ~35% |
| Code mode | 12-40s | 8-25s | ~35% |
| Token/task | 15-40K | 10-25K | ~33% |
| Cost/task | ~$0.006 | ~$0.003 | ~50% |

Key optimizations: merged review (1 call instead of 2), pipeline overlap (channel-based results), condensed context (symbols + 3 lines, ~80% savings), adaptive max_tokens, DeepSeek prompt caching.

## License

MIT
