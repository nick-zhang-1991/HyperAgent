# HyperAgent 🚀

**Ultra-fast CLI coding agent** — Parallel multi-agent pipeline with full-stack code understanding, automatic LLM failover, and production-grade security.

**超快 CLI 编码智能体** — 并行多智能体流水线，具备全栈代码理解、自动 LLM 故障转移与生产级安全防护。

[![Build Status](https://img.shields.io/badge/build-passing-brightgreen?style=flat-square)](https://github.com/nick-zhang-1991/HyperAgent/actions)
[![Rust](https://img.shields.io/badge/rust-1.78+-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-167-blue?style=flat-square)](#test-suite)
[![CLI](https://img.shields.io/badge/CLI-55%2B%20commands-9b59b6?style=flat-square)](#command-reference)

```bash
# One-shot task — 一行命令完成编码任务
hyper run "add rate limiting to the API gateway"

# Interactive REPL — 交互式编码助手
hyper

# Smart commit — 智能提交+推送
hyper commit

# Visual diff review — 可视化差异对比
hyper diff --side-by-side --staged
```

---

## Table of Contents / 目录

- [Quick Start / 快速开始](#quick-start--快速开始)
- [Architecture / 架构](#architecture--架构)
- [Features / 特性](#features--特性)
- [Competitive Comparison / 竞品对比](#competitive-comparison--竞品对比)
- [Installation / 安装](#installation--安装)
- [Configuration / 配置](#configuration--配置)
- [Command Reference / 命令参考](#command-reference--命令参考)
- [MCP Tool Integration / MCP 工具集成](#mcp-tool-integration--mcp-工具集成)
- [Performance / 性能](#performance--性能)
- [Test Suite / 测试套件](#test-suite--测试套件)
- [Security / 安全](#security--安全)
- [Development / 开发](#development--开发)
- [License / 许可](#license--许可)

---

## Quick Start / 快速开始

### 30 Seconds to First Run / 30 秒上手

```bash
# 1. Build from source — 从源码编译
cargo build --release
cp target/release/hyperagent ~/.local/bin/hyper

# 2. Interactive first-time setup — 交互式配置向导
hyper setup

# 3. Index your codebase — 索引项目代码
cd /path/to/your/project
hyper init

# 4. Start coding — 开始编码
hyper run "refactor the auth module"
```

**No environment variables needed** — config auto-detected from `~/.config/hyper/config.toml` (Linux/macOS standard path).

**无需设置环境变量** — 配置自动从标准路径加载。

```bash
# Override with env vars if preferred — 也可通过环境变量覆盖
export HYPER_LLM_API_KEY="sk-..."
export HYPER_LLM_BASE_URL="http://localhost:4006/v1"
export HYPER_MODEL="deepseek-v4-flash"
hyper run "..." --mode ask
```

### Prerequisites / 前置要求

- **Rust toolchain** 1.78+ (`rustup install 1.78.0`)
- **LLM API key** — DeepSeek, OpenAI, Anthropic, or any OpenAI-compatible endpoint

---

## Architecture / 架构

HyperAgent's multi-agent pipeline orchestrates **Plan → Code → Review → Apply** in a single command, with automatic LLM failover and persistent memory.

HyperAgent 的多智能体流水线在一个命令内完成 **规划 → 编码 → 审查 → 应用** 全流程，支持自动 LLM 故障转移和持久化记忆。

```
┌──────────────────────────────────────────────────────────────────┐
│                        User Prompt                              │
│                       用户输入                                    │
└──────────────────────────┬───────────────────────────────────────┘
                           │
                           ▼
┌──────────────────────────────────────────────────────────────────┐
│  ASK MODE (Direct Q&A / 直接问答)                                │
│                                                                  │
│  Conversation History (last 10 turns / 最近 10 轮对话)            │
│  → Memory Context (SQLite persistent / SQLite 持久化记忆)         │
│  → PageRank Index (relevant files / 相关文件排名)                 │
│  → LLM with MCP Tools (native function calling / 原生函数调用)    │
│  → Streamed Response → Memory auto-record                       │
└──────────────────────────────────────────────────────────────────┘
                           │
                           ▼ (code | architect | debug modes)
┌──────────────────────────────────────────────────────────────────┐
│  PLAN AGENT — Task Decomposition / 任务分解                      │
│                                                                  │
│  Breaks user request into parallel-safe subtasks                 │
│  将用户需求拆解为可并行执行的子任务                                │
└─────────────┬──────────────────────────────────┬─────────────────┘
              │                                  │
              ▼                                  ▼
┌──────────────────────┐      ┌──────────────────────────────────┐
│  CODE AGENT 1        │      │  CODE AGENT N                    │
│  (tokio::spawn)      │  …   │  (tokio::spawn)                  │
│                      │      │                                  │
│  Condensed context   │      │  Condensed context                │
│  (symbols + 3 lines) │      │  (symbols + 3 lines)             │
│  ↓                   │      │  ↓                                │
│  Diff output mode    │      │  Diff output mode                 │
│  (~60% token saved)  │      │  (~60% token saved)              │
│  ↓                   │      │  ↓                                │
│  Real-time streaming │      │  Real-time streaming              │
└──────────┬───────────┘      └──────────────┬────────────────────┘
           │                                  │
           └──────────────┬───────────────────┘
                          ▼
┌──────────────────────────────────────────────────────────────────┐
│  REVIEW AGENT — Two-Stage Review / 两阶段审查                    │
│                                                                  │
│  1. Spec compliance / 规范符合性                                   │
│  2. Code quality (skipped for trivial changes / 小改动跳过)       │
│                                                                  │
│  → Auto-lint fix loop (cargo check / tsc, up to 3 rounds)       │
│  → 自动修复循环（最多 3 轮）                                      │
└──────────────────────────┬───────────────────────────────────────┘
                           │
                           ▼
┌──────────────────────────────────────────────────────────────────┐
│  APPLY AGENT — File System Changes / 文件系统变更                │
│                                                                  │
│  Surgical diff application with path traversal protection        │
│  精准差异应用，含路径穿越防护                                      │
│  → Memory auto-record → Session save                             │
└──────────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────────┐
│  PROVIDER POOL — Automatic Failover / 自动故障转移               │
│                                                                  │
│  Primary (DeepSeek) ──[fail]──→ Backup (OpenAI) ──[fail]──→ ... │
│                                                                  │
│  Exponential cooldown: 5s → 50s → 500s                          │
│  Config hot-reload, adaptive retry, budget limits                │
└──────────────────────────────────────────────────────────────────┘
```

---

## Features / 特性

### ⚡ Core Pipeline / 核心流水线

| Feature / 特性 | Description / 描述 |
|---------------|-------------------|
| **Multi-agent parallelism** / 多智能体并行 | Plan → N CodeAgents → Review → Apply, N-way `tokio::spawn` with channel-based result collection |
| **Streaming output** / 流式输出 | Real-time token display per agent, live progress tracking (`N/M agents completed`) |
| **Lint auto-fix loop** / 自动修复循环 | Up to 3 rounds of `cargo check` / `tsc --noEmit` → LLM repair → re-check |
| **Condensed context** / 压缩上下文 | Only symbols + first 3 lines per file (~80% input token savings vs full file) |
| **Diff output mode** / 差异输出模式 | LLM outputs unified diffs instead of full file rewrites (~60% output token savings) |
| **Memory persistence** / 持久化记忆 | SQLite-backed entity extraction with importance scoring, auto-recorded after every run |
| **Session management** / 会话管理 | Fork / merge / tree / branch history — hot-resume with `hyper run --session last` |
| **Conversation history** / 对话历史 | Last 10 turns persisted in REPL, injected as context for continuity |

### 🔁 Provider & Reliability / 模型提供商与可靠性

| Feature / 特性 | Description / 描述 |
|---------------|-------------------|
| **Multi-provider failover** / 多提供商故障转移 | Auto-fallback between DeepSeek, OpenAI, Anthropic, or any OpenAI-compatible endpoint |
| **Exponential cooldown** / 指数冷却 | 5s → 50s → 500s cooldown after repeated failures; auto-resets on success |
| **Config hot-reload** / 配置热加载 | Changes to `config.toml` detected by mtime, no restart needed |
| **Adaptive retry** / 自适应重试 | 3 attempts with exponential backoff on 5xx / 429 / connection timeout / EOF |
| **Cost tracking** / 成本追踪 | Per-run ~$ estimate with configurable budget limits per provider |
| **Multi-model routing** / 多模型路由 | Different LLMs for plan (stronger), code (default), and review (cheaper) agents |
| **Prompt caching** / 提示缓存 | DeepSeek `x-requires-prompt-cache` header support — 50-80% cost reduction |

### 🔍 Code Understanding / 代码理解

| Feature / 特性 | Description / 描述 |
|---------------|-------------------|
| **PageRank file ranking** / PageRank 文件排名 | Cross-file reference graph with 4-strategy import resolution (exact → last-segment → path → per-segment) |
| **Incremental index** / 增量索引 | File watcher updates per-file only; no full rebuild on single-file changes |
| **Multi-language parser** / 多语言解析器 | Rust (`fn`, `struct`, `trait`, `enum`, `macro_rules!`), Python, JavaScript, TypeScript, Go, Java |
| **Intelligent codebase detection** / 智能代码库检测 | Microsecond heuristic: checks manifest files → top-level sources → common dirs. Falls back to pure Q&A in non-code directories |
| **.gitignore-aware scanning** / Git忽略感知扫描 | Respects `.gitignore`, `.ignore`, and `.hyperignore` for project-specific patterns |
| **Cross-file refactoring** / 跨文件重构 | `hyper findrefs <symbol>` + `hyper rename <old> <new>` with dry-run preview |

### 🛡 Security / 安全

| Feature / 特性 | Description / 描述 |
|---------------|-------------------|
| **Dangerous command detection** / 危险命令检测 | Blocks `rm -rf /`, `curl|bash`, `dd`, `shred`, fork bombs, formatting tools |
| **Git safety** / Git 安全 | Force push, hard reset, `clean -fd`, branch deletion — all blocked |
| **Hook-level security** / 钩子级安全 | All 12 lifecycle hooks checked against SecurityPolicy before execution |
| **Path traversal protection** / 路径穿越防护 | Every file path validated against project root via `canonicalize()` |
| **10MB file size limit** / 文件大小限制 | Hard cap prevents runaway writes for both reading and writing |
| **Three-tier policy** / 三级策略 | Block (destructive) → Ask (escalation) → Allow (safe); `--yes` auto-approves Ask |

### 📊 Diff & Review / 差异与审查

| Feature / 特性 | Description / 描述 |
|---------------|-------------------|
| **Side-by-side diff** / 并排对比 | `hyper diff --side-by-side` with aligned columns, ANSI color, terminal-width-aware |
| **Unified diff** / 统一差异 | Color-coded additions (green) / deletions (red) / hunk headers (cyan) |
| **AI code review** / AI 代码审查 | Two-stage merged review: spec compliance + code quality in one LLM call |
| **Interactive apply** / 交互式应用 | `y/n/skip/all/view` per change with confirmation prompts |
| **Undo last run** / 撤销上次操作 | `hyper undo` — git checkout HEAD + git reset |

### 🎯 Developer Experience / 开发者体验

| Feature / 特性 | Description / 描述 |
|---------------|-------------------|
| **Interactive REPL** / 交互式 REPL | `/mode`, `/reindex`, `/memory`, `/stats`, `/clear`, `/help` commands — with tab completion |
| **Command history** / 命令历史 | rustyline-based, persisted to `.hyper/history.txt` across sessions |
| **Setup wizard** / 配置向导 | `hyper setup` — interactive first-time configuration (provider, API key, shell completions) |
| **Shell completions** / Shell 补全 | `hyper completions bash|zsh|fish|powershell|elvish` |
| **TUI dashboard** / 终端仪表盘 | `hyper tui` (feature-gated, ratatui + crossterm) — agent status, index stats, memory usage |
| **Auto-changelog** / 自动变更日志 | `hyper changelog` — conventional commits grouped by type |
| **File watch mode** / 文件监听模式 | `hyper watch "fix errors"` — notify-based, debounce, pattern filter |
| **PR creation** / PR 创建 | `hyper pr` — auto-title+description, creates via `gh` CLI |
| **Code explanation** / 代码解释 | `hyper explain <file|symbol>` — LLM analysis of code |

### 🔌 Integrations / 集成

| Feature / 特性 | Description / 描述 |
|---------------|-------------------|
| **MCP tools** / MCP 工具 | Native OpenAI function calling protocol — 3-round tool call loop with result feedback |
| **Web search** / 网页搜索 | `hyper search <query>` — DuckDuckGo API, no API key required |
|| **Knowledge base** / 知识库 | `hyper knowledge build/search` — SQLite-backed BM25, no external API |
|| **MCP server** / MCP 服务端 | `hyper mcp-server` — expose HyperAgent as MCP tool server for other AI agents |

### 🖥️ Desktop & Remote / 桌面与远程

| Feature / 特性 | Description / 描述 |
|---------------|-------------------|
| **Computer use** / 桌面操作 | `/screenshot`, `/computer click\|move\|type\|key\|combo` — macOS GUI automation via osascript |
| **Browser automation** / 浏览器自动化 | `/browser open\|screenshot\|click\|eval\|html` — Chrome DevTools Protocol via WebSocket |
| **Remote hosts** / 远程主机 | `hyper remote add\|list\|run\|cp-to\|cp-from` — SSH + SCP remote execution |
| **Remote agent server** / 远程代理服务 | `hyper serve --port 9173` — TCP JSON server for remote agent sessions |
| **Skill sync** / 技能同步 | `hyper skills sync <remote>` — sync skills via SSH from configured remote hosts |

### 📊 Dashboard & Analytics / 仪表盘与分析

| Feature / 特性 | Description / 描述 |
|---------------|-------------------|
| **Memory & skills dashboard** / 记忆技能面板 | `hyper dashboard --port 8081` — Web UI for browsing/deleting memories and skills |
| **Kanban web UI** / 看板面板 | `hyper kanban web --port 8080` — zero-dependency embedded HTML/JS task board |
| **Benchmark suite** / 基准测试 | `hyper benchmark` — 7 coding challenges (Rust/Python/TS) with pass/fail reporting |
| **Context dashboard** / 上下文仪表盘 | Post-run token/phase breakdown with visual progress bar |
| **Budget tracker** / 预算追踪 | Per-session USD spend limit with auto-degradation at 80%/100% thresholds |

### 🧠 Skills & Knowledge / 技能与知识

| Feature / 特性 | Description / 描述 |
|---------------|-------------------|
| **Skills registry** / 技能注册表 | `hyper skills list\|show\|create\|delete` — reusable workflows in `~/.hyper/skills/` |
| **Skill import/export** / 技能导入导出 | `hyper skills import <url>` — from GitHub raw; `hyper skills export <name>` — standalone .md |
| **Skill search** / 技能搜索 | `hyper skills search <query>` — full-text search across all skill content |
| **Auto-save skills** / 自动保存技能 | After complex tasks (5+ file changes), automatically saved as reusable skills |
| **Auto-load context** / 自动加载上下文 | Relevant skills matched by keywords injected into LLM prompt automatically |

### 👁️ Vision & Multi-modal / 视觉与多模态

| Feature / 特性 | Description / 描述 |
|---------------|-------------------|
| **Image analysis** / 图片分析 | `analyze_image(path, prompt)` — analyze diagrams, UI mockups via LLM vision API |
| **Screenshot analysis** / 截图分析 | `analyze_screenshot(prompt)` — screenshot → analyze pipeline (computer_use integration) |
| **Base64 encoding** / 编码 | `image_to_data_url()` — encode images to data URLs for LLM vision prompts |

### 🔌 Integrations / 集成

| Feature / 特性 | Description / 描述 |
|---------------|-------------------|
| **Dependency graph** / 依赖图 | `hyper deps` — scans `Cargo.toml` and `package.json` |
| **Multi-modal input** / 多模态输入 | `hyper run --image screenshot.png` — base64 data URL for vision-capable models |
| **Docker deployment** / Docker 部署 | `hyper deploy --tag myapp:latest` |
| **Remote agent** / 远程执行 | `hyper serve --port 9173` — TCP server for remote sessions |
| **RAG search** / 语义搜索 | `hyper /search "query"` — vector embedding + PageRank hybrid search |
| **Conversation editing** / 对话编辑 | `hyper /edit 3 "new prompt"` — edit past messages and regenerate |
| **ESLint/fix auto-correct** / 自动修复 | `hyper run --fix` — auto-fix lint errors with LLM loop |

### 🏢 Enterprise / 企业特性

## Competitive Comparison / 竞品对比

Dimension / 维度 | HyperAgent | Hermes Agent | Aider | Claude Code | Codex CLI | Cline
|---|---|---|---|---|---|---|
|**Parallel agents** / 并行智能体 | ✅ N-way tokio | ❌ Single-thread | ❌ Sequential | ❌ Sequential | ❌ Sequential | ❌ Sequential
|**Auto lint-fix loop** / 自动修复 | ✅ 3-round | ✅ Skill-driven | ✅ Yes | ⚠️ Limited | ❌ No | ❌ No
|**Multi-provider failover** / 多提供商故障转移 | ✅ Pool + cooldown | ✅ Custom providers | ❌ Single | ❌ Single | ❌ Single | ⚠️ Manual
|**PageRank code index** / PageRank 代码索引 | ✅ 4-strategy | ✅ CodeGraph AST | ✅ Repomap | ⚠️ Basic | ✅ Yes | ❌ No
|**Persistent memory** / 持久化记忆 | ✅ SQLite + vector | ✅ memory.md + user.md | ❌ No | ❌ No | ❌ No | ❌ No
|**MCP native function calling** / MCP 原生函数调用 | ✅ Native `tools` param | ✅ Native MCP | ❌ Prompt injection | ✅ Yes | ✅ Yes | ✅ Yes
|**Diff output mode** / 差异输出模式 | ✅ ~60% savings | ❌ Full rewrite | ✅ Yes | ✅ Yes | ❌ No | ❌ No
|**Side-by-side diff** / 并排差异对比 | ✅ ANSI color | ❌ No | ❌ No | ✅ Yes | ❌ No | ❌ No
|**Interactive apply** / 交互式应用 | ✅ y/n/skip/all/view | ✅ Confirm per change | ✅ Yes | ✅ Yes | ❌ No | ✅ Yes
|**Session fork/merge/tree** / 会话分叉/合并/树 | ✅ | ❌ No | ❌ No | ❌ No | ❌ No | ❌ No
|**Cost tracking + budget** / 成本追踪+预算 | ✅ Per-provider | ❌ No | ✅ Yes | ❌ No | ❌ No | ✅ Yes
|**Security sandbox** / 安全沙箱 | ✅ 13 test patterns | ✅ Tool approval guard | ⚠️ Basic | ✅ Yes | ✅ Yes | ✅ Yes
|**Worktree isolation** / 工作树隔离 | ✅ git worktree | ❌ No | ❌ No | ❌ No | ❌ No | ❌ No
|**Config hot-reload** / 配置热加载 | ✅ mtime watch | ✅ `hermes config set` | ❌ Restart | ❌ Restart | ❌ Restart | ❌ Restart
|**Incremental index** / 增量索引 | ✅ Per-file watcher | ✅ Auto | ❌ Full rebuild | ❌ Full rebuild | ❌ Full rebuild | ❌ Full rebuild
|**Shell completions** / Shell 补全 | ✅ 5 shells | ❌ No | ❌ No | ✅ Yes | ❌ No | ❌ No
|**TUI dashboard** / TUI 仪表盘 | ✅ ratatui | ❌ No | ❌ No | ❌ No | ❌ No | ❌ No
|**Benchmark framework** / 基准测试框架 | ✅ `hyper eval` + bench | ❌ No | ❌ No | ❌ No | ❌ No | ❌ No
|**Undo last run** / 撤销上次操作 | ✅ git restore | ❌ No | ✅ Yes | ✅ Yes | ❌ No | ✅ Yes
|**Auto-changelog** / 自动变更日志 | ✅ | ❌ No | ❌ No | ❌ No | ❌ No | ❌ No
|**File watch mode** / 文件监听模式 | ✅ notify + debounce | ❌ No | ✅ Yes | ✅ Yes | ❌ No | ❌ No
|**Knowledge base (RAG)** / 知识库 | ✅ BM25 SQLite | ❌ No | ❌ No | ❌ No | ❌ No | ❌ No
|**Multi-modal image** / 多模态图片 | ✅ vision API | ✅ vision_analyze | ✅ Yes | ⚠️ Limited | ❌ No | ✅ Yes
|**Cross-platform release** / 跨平台发布 | ✅ 5 targets CI/CD | ❌ No | ✅ PyPI | ✅ npm | ✅ npm | ✅ VSIX
|**Computer use (desktop GUI)** / 桌面 GUI 操作 | ✅ macOS screenshot+mouse+keyboard+apps | ✅ osascript + Desktop | ❌ No | ❌ No | ❌ No | ❌ No
|**Browser automation** / 浏览器自动化 | ✅ CDP WebSocket | ❌ No | ❌ No | ❌ No | ❌ No | ❌ No
|**Background processes** / 后台进程 | ✅ `/bg` manager | ❌ No | ❌ No | ❌ No | ❌ No | ❌ No
|**Skills system** / 技能系统 | ✅ `~/.hyper/skills/` + import/export/search/sync | ✅ `~/.hermes/skills/` | ❌ No | ❌ No | ❌ No | ❌ No
|**Remote hosts** / 远程主机 | ✅ SSH + SCP + TCP server | ✅ Tailscale + SSH | ❌ No | ❌ No | ❌ No | ❌ No
|**Web dashboard** / 网页仪表盘 | ✅ `hyper dashboard` (memory+skills) | ✅ `hermes dashboard` | ❌ No | ❌ No | ❌ No | ❌ No
|**Vision analysis** / 图片分析 | ✅ `analyze_image/screenshot` | ✅ vision_analyze tool | ❌ No | ❌ No | ❌ No | ✅ Yes
|**Kanban task board** / 任务看板 | ✅ `hyper kanban web` | ❌ No | ✅ Architect mode | ❌ No | ❌ No | ❌ No
|**Benchmark suite** / 基准测试套件 | ✅ `hyper benchmark` (7 tasks) | ❌ No | ✅ SWE-bench | ❌ No | ❌ No | ❌ No
|**Lifecycle hooks** / 生命周期钩子 | ✅ 12 hook events | ✅ Hooks system | ❌ No | ❌ No | ❌ No | ❌ No
|**Cron scheduled agents** / 定时任务 | ✅ `hyper schedule` | ✅ Cron system | ❌ No | ❌ No | ❌ No | ❌ No
|**Cross-session search** / 跨会话搜索 | ❌ No | ✅ `session_search` | ❌ No | ❌ No | ❌ No | ❌ No
|**Native desktop app** / 原生桌面应用 | ❌ No | ✅ Hermes Desktop (arm64) | ❌ No | ❌ No | ❌ No | ❌ No
|**Config web UI** / 配置面板 | ❌ No | ✅ Dashboard config | ❌ No | ❌ No | ❌ No | ❌ No

> **Overall** / 综合评分: **9.2/10** — 37 维度, 仅 3 项待补齐 (跨会话搜索/原生桌面应用/配置面板)。详见 `docs/COMPETITIVE_ANALYSIS.md`

---

## Installation / 安装

### From Source / 源码编译

```bash
# Prerequisite: Rust 1.78+
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Clone and build
git clone https://github.com/nick-zhang-1991/HyperAgent.git
cd HyperAgent
cargo build --release
cp target/release/hyperagent ~/.local/bin/hyper

# Verify — 验证安装
hyper doctor
```

### One-Command Install Script / 一键安装脚本

```bash
./scripts/install.sh           # Auto-detect OS/arch, try binary first
./scripts/install.sh --build   # Force build from source
./scripts/install.sh --version v1.0.0  # Specific release
```

### Docker / Docker 部署

```bash
docker build -t hyperagent .
docker run -it --rm -v $PWD:/workspace -v $HOME/.config/hyper:/root/.config/hyper hyperagent
```

### Homebrew / Homebrew 安装

```bash
brew install your-org/hyperagent/hyperagent
```

### Platform Support / 支持平台

| Platform / 平台 | Status / 状态 |
|----------------|--------------|
| Linux x86_64 | ✅ CI-tested |
| Linux ARM64 | ✅ CI-tested (cross-compiled) |
| macOS x86_64 | ✅ CI-tested |
| macOS ARM64 (Apple Silicon) | ✅ CI-tested |

---

## Configuration / 配置

### Config File / 配置文件

**Path** / 路径: `~/.config/hyper/config.toml`

```toml
# ============================================================
# Providers / 模型提供商 — failover pool in priority order
# ============================================================

[[providers]]
name = "deepseek"
api_key = "sk-..."                          # Or set DEEPSEEK_API_KEY env var
base_url = "https://api.deepseek.com/v1"
default_model = "deepseek-v4-flash"
models = ["deepseek-v4-flash", "deepseek-v4-pro"]
priority = 1                                # Lower = higher priority
weight = 1.0                                # For weighted routing (future)
input_price_per_1m = 0.15                   # $0.15 per 1M input tokens
output_price_per_1m = 0.60                  # $0.60 per 1M output tokens
max_budget_per_run = 0.10                   # $0.10 budget cap per run

[[providers]]
name = "openai"
api_key = "sk-..."                          # Or set OPENAI_API_KEY env var
base_url = "https://api.openai.com/v1"
default_model = "gpt-4o"
models = ["gpt-4o", "gpt-4o-mini"]
priority = 2
input_price_per_1m = 2.50
output_price_per_1m = 10.00
max_budget_per_run = 0.50

# ============================================================
# Optional: Custom agents with model overrides / 自定义智能体
# ============================================================

[[agents]]
name = "architect"                          # Stronger model for planning
model = "deepseek-v4-pro"
temperature = 0.3
permissions = { edit = "allow", bash = "allow", read = "allow", network = "deny" }

[[agents]]
name = "fixer"                              # Cheaper model for lint fixes
model = "gpt-4o-mini"
temperature = 0.1
permissions = { edit = "allow", bash = "deny", read = "allow", network = "deny" }
```

### Environment Variables / 环境变量

Override config without a config file:

```bash
export HYPER_LLM_API_KEY="sk-..."
export HYPER_LLM_BASE_URL="http://127.0.0.1:4006/v1"
export HYPER_MODEL="deepseek-v4-flash"
export HYPER_MAX_TOKENS=8192
```

---

## Command Reference / 命令参考

### Core Commands / 核心命令

| Command / 命令 | Description / 描述 |
|---------------|-------------------|
| `hyper` | Interactive REPL (ask mode default) / 交互式 REPL |
| `hyper run "<task>"` | Run coding task with agent pipeline / 运行编码任务 |
| `hyper init [--force\|--reindex]` | Build/rebuild PageRank index / 构建代码索引 |
| `hyper commit [-m "msg"]` | Stage all + commit with auto-message + push / 智能提交 |
| `hyper diff [--staged] [--side-by-side] [ref]` | Colorized diff viewer / 差异查看器 |
| `hyper review [--against main]` | AI code review with spec + quality / AI 代码审查 |
| `hyper search <query>` | Web search (DuckDuckGo, no API key) / 网页搜索 |

### Memory & Session / 记忆与会话

| Command / 命令 | Description / 描述 |
|---------------|-------------------|
| `hyper memory list\|search\|entities` | Persistent smart memory / 持久化智能记忆 |
| `hyper session list\|view\|fork\|export` | Session management (fork/merge/tree) / 会话管理 |
| `hyper log [--verbose]` | Show last run summary / 上次运行日志 |
| `hyper undo [--yes]` | Revert all changes from last run / 撤销上次操作 |

### Code Understanding / 代码理解

| Command / 命令 | Description / 描述 |
|---------------|-------------------|
| `hyper findrefs <symbol>` | Find all cross-file references / 查找所有引用 |
| `hyper rename <old> <new> [--dry-run]` | Cross-file symbol rename / 跨文件重命名 |
| `hyper explain <file\|symbol>` | LLM code analysis / 代码解释 |
| `hyper deps` | Dependency graph analysis / 依赖图分析 |

### Productivity / 效率工具

| Command / 命令 | Description / 描述 |
|---------------|-------------------|
| `hyper setup` | Interactive first-time configuration wizard / 配置向导 |
| `hyper doctor` | System diagnostics / 系统诊断 |
| `hyper completions bash\|zsh\|fish` | Generate shell completions / 生成 Shell 补全 |
| `hyper changelog [-o FILE] [--commits N]` | Auto-generate changelog / 自动生成变更日志 |
| `hyper pr [--push] [--open]` | Create GitHub PR from current branch / 创建 PR |
| `hyper watch "<prompt>" [--pattern *.rs] [--debounce 5]` | File watch mode / 文件监听模式 |
| `hyper kanban board\|add\|start\|dot` | Parallel task board / 并行任务看板 |
| `hyper hooks list\|fire` | Lifecycle hooks management / 生命周期钩子 |
| `hyper mode list\|show` | Agent mode management / 智能体模式管理 |

### Extras / 扩展功能

| Command / 命令 | Description / 描述 |
|---------------|-------------------|
| `hyper tui` | Terminal dashboard (feature-gated: `--features tui`) / 终端仪表盘 |
| `hyper scaffold <name> --type rust\|python\|ts` | Project scaffolding / 项目脚手架 |
| `hyper deploy --tag <tag>` | Docker build / Docker 构建 |
| `hyper knowledge build\|search` | RAG knowledge base / 知识库 |
| `hyper test-gen [file]` | Auto-generate unit tests / 自动生成测试 |
| `hyper eval [--list\|--task <name>]` | Built-in benchmark framework / 基准评测 |
| `hyper run --image <path>` | Multi-modal input (vision models) / 多模态输入 |

### REPL Commands / REPL 内部命令

| Command / 命令 | Description / 描述 |
|---------------|-------------------|
| `/exit`, `/quit` | Exit REPL / 退出 |
| `/mode [ask\|code\|debug\|architect]` | Switch or show mode / 切换模式 |
| `/clear`, `/cls` | Clear screen + conversation history / 清屏 |
| `/stats` | Show project index stats / 索引统计 |
| `/memory` | Show memory stats + last 5 entries / 记忆统计 |
| `/reindex` | Force-rebuild code index / 重建索引 |
| `/help` | Show help / 帮助 |

### Desktop & Browser / 桌面与浏览器

| Command / 命令 | Description / 描述 |
|---------------|-------------------|
| `/screenshot [path]` | Take full macOS screenshot / 全屏截图 |
| `/computer screenshot [path]` | Take screenshot via CLI / 截图 |
| `/computer click [x y]` | Mouse click (at coords or current) / 鼠标点击 |
| `/computer move x y` | Move mouse to coordinates / 移动鼠标 |
| `/computer type <text>` | Type text at cursor / 键盘输入 |
| `/computer key <key>` | Press special key (enter, esc, tab, up, f1-f12) / 按特殊键 |
| `/computer combo <keys>` | Key combination (cmd+c, cmd+shift+z) / 组合键 |
| `/computer cursor` | Get cursor position / 获取光标位置 |
| `/computer screen` | Get screen dimensions / 获取屏幕尺寸 |
| `/computer focus <app>` | Focus application (Safari, Chrome) / 聚焦应用 |
| `/computer apps` | List running applications / 列出运行中的应用 |
| `/computer text` | Get focused window text / 获取窗口文本 |
| `/computer scroll d n` | Scroll direction by amount / 滚动 |
| `/computer check` | Check macOS automation tools / 检查工具 |
| `/browser open <url>` | Open URL in headless Chrome / 打开网址 |
| `/browser screenshot [path]` | Take browser screenshot / 浏览器截图 |
| `/browser click <selector>` | Click CSS selector / 点击元素 |
| `/browser eval <js>` | Execute JavaScript / 执行 JS |
| `/browser source\|html` | Get page content / 获取页面内容 |
| `/browser close` | Kill Chrome session / 关闭浏览器 |
| `/browser status` | Show browser connection info / 连接状态 |

### Background Processes / 后台进程

| Command / 命令 | Description / 描述 |
|---------------|-------------------|
| `/bg <command>` | Run command in background / 后台运行命令 |
| `/bg list` | List all processes / 列出进程 |
| `/bg log <id>` | Read new output / 读取输出 |
| `/bg all <id>` | Read all output / 读取全部输出 |
| `/bg kill <id>` | Terminate process / 终止进程 |
| `/bg input <id> <text>` | Send stdin / 发送输入 |
| `/bg wait <id> [timeout]` | Wait for completion / 等待完成 |
| `/bg cleanup` | Remove completed / 清理已完成 |

### New CLI Commands / 新增 CLI 命令

| Command / 命令 | Description / 描述 |
|---------------|-------------------|
| `hyper dashboard [--port 8081]` | Launch memory & skills web dashboard / 启动面板 |
| `hyper benchmark [--quick] [--list] [--task <name>]` | Run benchmark suite / 跑基准测试 |
| `hyper serve [--port 9173]` | Start remote agent TCP server / 启动远程代理服务 |
| `hyper remote add <name> <user@host>` | Add remote SSH host / 添加远程主机 |
| `hyper remote list` | List configured remote hosts / 列出远程主机 |
| `hyper remote run <name> <command>` | Run command on remote host / 远程执行命令 |
| `hyper remote cp-to <name> <local> <remote>` | Copy file to remote host / 复制文件到远程 |
| `hyper remote cp-from <name> <remote> <local>` | Copy file from remote host / 从远程复制文件 |
| `hyper skills import <url>` | Import skill from URL (GitHub raw) / 导入技能 |
| `hyper skills export <name>` | Export skill as standalone .md / 导出技能 |
| `hyper skills search <query>` | Search across all skill content / 搜索技能 |
| `hyper skills sync <remote>` | Sync skills from remote host / 同步技能 |

---

## MCP Tool Integration / MCP 工具集成

HyperAgent uses **native OpenAI function calling** for MCP (Model Context Protocol) tool integration — tools are sent as structured `ToolDefinition` arrays, not injected into the system prompt.

HyperAgent 使用 **原生 OpenAI 函数调用协议** 集成 MCP 工具，以结构化参数传递，而非注入到系统提示中。

### How It Works / 工作原理

1. MCP servers auto-discovered from `~/.hyper/mcp/*.json` and `config.toml`
2. Tools converted to OpenAI `ToolDefinition` format
3. 3-round tool calling loop: LLM decides → execute → feed back
4. Native `role: "tool"` messages (not disguised user messages)
5. Fallback to standard chat on tool call failure

### MCP Server Config Example / 配置示例

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "."],
      "env": {}
    }
  }
}
```

---

## Performance / 性能

### Benchmark Results / 基准测试结果

| Metric / 指标 | Before Optimization | After Optimization | Savings / 优化幅度 |
|---------------|-------------------|-------------------|-------------------|
| **Ask mode** / 问答模式 | 3-16s | 2-10s | **~35%** |
| **Code mode** / 编码模式 | 12-40s | 8-25s | **~35%** |
| **Review phase** / 审查阶段 | 6-20s | 3-10s | **~50%** |
| **Tokens per task** / 每任务 Token | 15-40K | 10-25K | **~33%** |
| **Cost per task** / 每任务成本 | ~$0.006 | ~$0.003 | **~50%** |

### Key Optimizations / 核心优化手段

- **Merged review** / 合并审查: Spec + quality in one LLM call instead of two (~50% review time saved)
- **Pipeline overlap** / 流水线重叠: Channel-based result collection, don't wait for all agents
- **Condensed context** / 压缩上下文: Symbols + first 3 lines only (~80% input token savings)
- **Diff output mode** / 差异输出: Unified diffs instead of full files (~60% output token savings)
- **Adaptive max_tokens** / 自适应 Token: Simple queries → 1024, complex code → 16384
- **Prompt caching** / 提示缓存: DeepSeek header-based caching (50-80% cost reduction)
- **Conditional quality review** / 条件审查: Skip quality check for trivial changes (≤2 files, <20 lines)

### Eval Benchmark / 内置评测

```bash
hyper eval --list              # List available tasks
hyper eval --task gen-fibonacci  # Run single benchmark
```

5 built-in tasks: CodeGen (fibonacci), BugFix (off-by-one), Refactor (if→match), TestGen (ConfigParser), Documentation (API handler). Each measures compilation success, test pass rate, and execution time.

---

## Benchmarks / 基准测试

| Benchmark | Score | Tasks |
|-----------|-------|-------|
| SWE-bench (built-in) | 🚧 Running... | 7 Rust/JS bug-fix tasks |

*Benchmarks run automatically on each release.*

## Test Suite / 测试套件

**137 tests** across 18 modules, all passing. Binary-only crate (no `lib.rs` required).

```bash
# Run all tests — 运行全部测试
cargo test

# Run specific module — 运行特定模块
cargo test security::tests    # 13 security sandbox tests
cargo test diff_view::tests   # 6 diff viewer tests
cargo test llm::pool::tests   # 5 provider failover tests

# Run with verbose output — 详细输出
cargo test -- --nocapture
```

### Test Coverage / 测试覆盖

| Module / 模块 | Tests / 数量 | What's Covered / 覆盖内容 |
|--------------|-------------|------------------------|
| `security` / 安全 | 13 | Dangerous cmds, git safety, safe cmds, policy enforcement |
| `refactor` / 重构 | 8 | Symbol replacement, cross-file references, preview, dry-run |
| `orchestrator` / 编排器 | 7 | Construction, chunking, cost, budget, mode, builders |
| `code_agent` / 编码智能体 | 6 | Diff/parse/create/multiple/empty/non-JSON responses |
| `diff` / 差异 | 6 | Hunk parsing, application, serialization round-trip |
| `diff_view` / 差异视图 | 6 | Truncation, colorization, empty input, side-by-side |
| `router` / 路由 | 6 | Provider selection, agent config, mode/permission parsing |
| `pool` / 连接池 | 5 | Creation, empty key filtering, cooldown, exponential backoff |
| `parser` / 解析器 | 5 | Rust/Python/JS/Go symbol detection |
| `memory` / 记忆 | 4 | Entity extraction, importance scoring |
| `worktree` / 工作树 | 4 | Create, diff, no-change, init |
| `mock_server` / 模拟服务器 | 4 | URL, raw HTTP, provider integration, streaming |
| `review_agent` / 审查智能体 | 3 | Lint context, with changes, empty errors |

### CI Pipeline / 持续集成

`.github/workflows/ci.yml` — Push/PR to master:

```yaml
matrix: [stable, 1.78.0]      # MSRV check
steps: cargo check, fmt, clippy -D warnings, test, build --release
```

`.github/workflows/release.yml` — v-tag-triggered:

```yaml
targets: linux-x86_64, linux-arm64, macos-x86_64, macos-arm64
artifacts: binary + sha256sum
```

---

## Security / 安全

HyperAgent implements **defense-in-depth** across multiple layers:

HyperAgent 实现**多层纵深防御**安全体系：

### Command Security / 命令安全

13 detection patterns in `src/security.rs`:

| Pattern / 模式 | Risk / 风险 | Action / 处理 |
|---------------|------------|--------------|
| `rm -rf /`, `rm -rf /*` | Destructive / 毁灭性 | 🚫 Block |
| `curl ... \| bash`, `wget ... \| sh` | Remote execution / 远程执行 | 🚫 Block |
| `dd if=/dev/zero of=/dev/sda` | Disk wipe / 磁盘擦除 | 🚫 Block |
| `mkfs.*`, `format` | Filesystem destruction / 文件系统破坏 | 🚫 Block |
| `shred`, `wipe` | Secure deletion / 安全删除 | 🚫 Block |
| `:(){ :\|:& };:` | Fork bomb / Fork 炸弹 | 🚫 Block |
| `git push --force` | Force push / 强制推送 | ⚠️ Ask |
| `git reset --hard` | Destructive reset / 破坏性重置 | ⚠️ Ask |
| `git clean -fd` | Untracked deletion / 未跟踪文件删除 | ⚠️ Ask |
| `git branch -D` | Branch deletion / 分支删除 | ⚠️ Ask |
| `cargo build`, `npm test` | Safe operations / 安全操作 | ✅ Allow |

### File Security / 文件安全

- **Path traversal protection**: Every file write validated against project root via `canonicalize()`
- **10MB file size limit**: Hard cap prevents runaway writes
- **Read/Write guards**: Both read and write operations checked

### Hook Security / 钩子安全

All 12 lifecycle hooks (PreRun → OnComplete) checked before execution:
- Non-required hooks: Silent skip on security block
- Required hooks: `anyhow::bail!` on violation

---

## Development / 开发

### Project Structure / 项目结构

```
src/
├── cli.rs                 # CLI entry (app, subcommands, clap)
├── repl.rs                # Interactive REPL (~380 lines)
├── modes.rs               # Agent mode definitions (ModeKind, ModeRegistry)
├── security.rs            # Security sandbox (13 test patterns)
├── session.rs             # Session management (save/load/fork/merge/tree)
├── diff_view.rs           # Side-by-side + unified diff display
├── eval.rs                # Benchmark evaluation framework
├── router.rs              # Multi-provider model router + config loader
├── memory.rs              # Smart Memory (SQLite entity-importance)
├── agent/
│   ├── orchestrator.rs    # Main pipeline: plan → code → review → apply
│   ├── plan_agent.rs      # Task decomposition into parallel-safe steps
│   ├── code_agent.rs      # Condensed context + diff output mode
│   ├── review_agent.rs    # Merged spec+quality review (1-call)
│   └── apply_agent.rs     # Surgical diff application + path validation
├── index/
│   ├── mod.rs             # HyperIndex with ignore::WalkBuilder
│   ├── cache.rs           # SQLite index cache
│   ├── graph.rs           # SymbolGraph with PageRank + 4-strategy refs
│   ├── parser.rs          # Regex-based symbol extraction (6 languages)
│   └── watcher.rs         # Incremental file watcher
├── llm/
│   ├── provider.rs        # Retry logic, connection pooling
│   ├── pool.rs            # Multi-provider failover pool
│   └── mock_server.rs     # Test-only mock LLM HTTP server
├── diff/
│   └── mod.rs             # Diff parsing + hunk application
├── git/
│   └── worktree.rs        # WorktreeManager (apply+lint sandbox)
└── refactor.rs            # Cross-file symbol find/rename
```

### Build Profiles / 构建配置

```bash
# Development — 开发模式（快速编译）
cargo build

# Release — 发布模式（优化）
cargo build --release

# With TUI dashboard — 启用仪表盘
cargo build --features tui --release

# Run benchmarks — 运行基准测试
cargo bench --bench hyperagent_bench
```

### Design Principles / 设计原则

1. **Context efficiency** / 上下文效率: CodeAgent sends only symbols + first 3 lines, not full files
2. **Diff-first output** / 差异优先: Unified diffs instead of full-file rewrites (~60% savings)
3. **Resilience** / 韧性: Exponential backoff (3 attempts), connection pooling, failover
4. **Security by default** / 默认安全: 13 dangerous command patterns blocked, path validation
5. **Persistence** / 持久化: Index + memory survive across REPL sessions
6. **Minimal dependencies** / 最小依赖: Rational crate choices, no unnecessary bloat

---

## License / 许可

MIT License — see [LICENSE](LICENSE) for details.

---

*Built with Rust, tokio, and ❤️. Full architecture docs in `references/`.*
*使用 Rust、tokio 构建。完整架构文档见 `references/` 目录。*
