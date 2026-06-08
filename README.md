# HyperAgent 🚀

**通用 AI Agent** — 编码、分析、自动化、协作一体。并行多智能体架构，内置世界级记忆系统。

**General-Purpose AI Agent** — Code, analyze, automate, collaborate. Parallel multi-agent architecture with world-class memory.

[![CI](https://github.com/nick-zhang-1991/HyperAgent/actions/workflows/ci.yml/badge.svg)](https://github.com/nick-zhang-1991/HyperAgent/actions)
[![Release](https://img.shields.io/badge/release-v0.2.0-blue?style=flat-square)](https://github.com/nick-zhang-1991/HyperAgent/releases/tag/v0.2.0)
[![Rust](https://img.shields.io/badge/rust-1.82+-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)
[![Stars](https://img.shields.io/github/stars/nick-zhang-1991/HyperAgent?style=flat-square)](https://github.com/nick-zhang-1991/HyperAgent/stargazers)
[![i18n](https://img.shields.io/badge/i18n-20_languages-blue?style=flat-square)](https://github.com/nick-zhang-1991/HyperAgent)

> ⚡ 启动 <0.3s · 🧠 越用越聪明的跨项目记忆 · 🐝 多 Agent 并行 · 🌐 20语言支持

```bash
# 编码任务
hyper run "用 Rust 实现一个线程安全的 LRU 缓存"

# 深度分析
hyper analyze                    # 安全、复杂度、死代码全面审计

# 自动化
hyper swarm "构建 REST API + 认证 + 限流 + 测试"    # 多 Agent 并行

# 研究搜索
hyper run "对比 2024 年 Rust vs Go 的性能基准测试" --mode ask

# DevOps
hyper ci-fix build.log --push    # CI 管道自动修复
hyper run "优化 Dockerfile 减小镜像体积 50%"

# 自我进化
hyper feedback good "遵循了 Rust 社区约定"
hyper feedback bad "不应该在库代码里用 unwrap()"
```

---

## Installation / 安装

```bash
# macOS/Linux
brew install nick-zhang-1991/hyperagent/hyperagent

# 或 cargo-binstall（推荐，秒装）
cargo binstall hyperagent

# 或源码编译
cargo install hyperagent

# 或一键脚本
curl -fsSL https://raw.githubusercontent.com/nick-zhang-1991/HyperAgent/main/scripts/install.sh | bash
```

[中文用户请查看 README_zh-CN.md](README_zh-CN.md) | `export HYPER_LANG=zh-CN`

20 languages supported: `en` `zh-CN` `es` `ar` `pt` `id` `fr` `ja` `de` `ru` `ko` `vi` `it` `tr` `pl` `uk` `nl` `th` `bn` `hi`

---

## What Can It Do? / 能做什么

| 场景 | 命令示例 |
|------|---------|
| 🖥️ **编码** | `hyper run "添加 JWT 认证中间件"` |
| 🔍 **分析** | `hyper analyze` — 安全漏洞 / 复杂度过高 / 死代码 |
| 🤖 **自动化** | `hyper swarm "拆分单体为微服务"` — 多 Agent 并行开工 |
| 🧪 **测试** | `hyper run "为 UserService 写单元测试"` |
| 📚 **研究** | `hyper run "Rust async vs Go goroutine 深度对比" --mode ask` |
| 🔧 **DevOps** | `hyper ci-fix ci.log` — CI 挂了自动修 |
| 🔒 **安全** | `hyper run "审计代码库的安全漏洞"` |
| 📦 **部署** | `hyper run "生成 Kubernetes deployment 配置"` |
| 🌍 **维护** | 自动分析 → 发现 bug → 生成 PR → 你审批 |
| 🧠 **进化** | `hyper feedback good/bad` — 告诉 Agent 什么做对了、什么需要改 |

---

## Feature Matrix / 功能矩阵

### CLI Commands (20+)

| Command | 中文 | What it does |
|---------|------|-------------|
| `hyper run` | 运行任务 | Execute any task with multi-agent pipeline |
| `hyper init` | 初始化 | Onboarding wizard + code index |
| `hyper serve` | 启动服务 | Web API server (SSE streaming) |
| `hyper analyze` | 代码分析 | Security, complexity, dead code audit |
| `hyper swarm` | 集群协作 | Parallel multi-agent execution |
| `hyper ci-fix` | CI 修复 | Auto-fix CI pipeline failures |
| `hyper review` | 代码审查 | Review staged changes |
| `hyper doctor` | 诊断 | System diagnostics |
| `hyper memory global` | 全局记忆 | Cross-project knowledge |
| `hyper session share` | 会话共享 | Share session via token |
| `hyper feedback` | 反馈训练 | Teach the agent (RLHF-lite) |
| `hyper skill` | 技能市场 | Install/search/create community skills |
| `hyper bench memory` | 性能测试 | Memory system benchmark |
| `hyper eval` | 自评估 | Self-evaluation suite |

### Unique Selling Points / 独家能力

| 能力 | HyperAgent | Claude Code | Aider | Cursor | Devin |
|------|-----------|------------|-------|--------|-------|
| 通用任务（非仅编码） | ✅ | ❌ 仅编码 | ❌ 仅编码 | ❌ 仅编码 | ✅ |
| 多 Agent 并行 | ✅ swarm | ❌ | ❌ | ❌ | ✅ |
| 跨项目全局记忆 | ✅ | ❌ | ❌ | ❌ | ❌ |
| 自纠错学习 | ✅ feedback | ❌ | ❌ | ❌ | ❌ |
| 深度代码分析 | ✅ analyze | ❌ | ❌ | ❌ | ❌ |
| CI 自动修复 | ✅ ci-fix | ❌ | ❌ | ❌ | ✅ |
| 中文原生 | ✅ zh-CN + 20语言 | ❌ | ❌ | ❌ | ❌ |
| Skill 市场 | ✅ | ❌ | ❌ | ❌ | ✅ |
| SSE 流式 | ✅ | ✅ | ❌ | ✅ | ✅ |
| VS Code 扩展 | ✅ v0.2 | ❌ | ✅ | ✅ | ✅ |
| Desktop 原生 | ✅ Tauri | ❌ | ❌ | ✅ | ✅ |
| Web UI | ✅ | ❌ | ❌ | ✅ | ✅ |
| 会话共享 | ✅ token | ❌ | ❌ | ✅ | ✅ |
| Docker Sandbox | ✅ | ❌ | ✅ | ❌ | ✅ |

---

## Architecture / 架构

```
User Request
    ↓
CLI · Web UI · Desktop · VS Code
    ↓
hyper serve (axum + SSE streaming)
    ↓
Agent Pipeline (Plan → Code → Review → Apply → Fix)
    ↓                    ↓                    ↓
Memory System     LLM Provider Pool     Tool System
(14 types,        (failover,            (MCP, Web Search,
 FTS5, embed,      health check,         Browser, Desktop,
 auto-prune,       circuit breaker)      Sandbox, REPL)
 global memory)
    ↓                    ↓                    ↓
Global Memory     Community Skills     Plugin System
(cross-project    (marketplace,        (hooks, events,
 knowledge)        install/share)       extensions)
```

[Full Mermaid diagrams → docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)

---

## Quick Start / 快速开始

```bash
# 1. 安装
brew install nick-zhang-1991/hyperagent/hyperagent

# 2. 配置 LLM
export HYPER_LLM_API_KEY=***   export HYPER_LLM_MODEL="gpt-4o"
export HYPER_LLM_BASE_URL="https://api.openai.com/v1"

# 3. 中文界面
export HYPER_LANG=zh-CN

# 4. 初始化项目
cd your-project && hyper init

# 5. 开始使用
hyper run "给我解释这个项目的架构"
hyper analyze                     # 全面审计
hyper serve                       # 启动 Web 界面
```

---

## Community / 社区

- [Contributing Guide](CONTRIBUTING.md)
- [Security Policy](SECURITY.md)
- [Code of Conduct](CODE_OF_CONDUCT.md)
- [Changelog](CHANGELOG.md)

### Skills / 技能市场

Share your expertise: `hyper skill create` → edit → GitHub Gist → `hyper skill install <gist-url>`

Browse community skills:
- `hyper skill search rust`  — 搜索
- `hyper skill list`          — 已安装
- [skills/](skills/)          — 种子技能库

---

## Documentation / 文档

- [Architecture](docs/ARCHITECTURE.md)
- [Competitive Analysis](docs/COMPETITIVE_ANALYSIS.md)
- [Roadmap](docs/ROADMAP_2026-06-08.md)
- [Chinese README / 中文文档](README_zh-CN.md)

---

## License

MIT © 2026 HyperAgent Contributors
