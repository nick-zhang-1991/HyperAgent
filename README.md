# HyperAgent 🚀

**General-Purpose AI Agent** — Code, analyze, automate, collaborate. Parallel multi-agent architecture with world-class memory.

[![CI](https://github.com/nick-zhang-1991/HyperAgent/actions/workflows/ci.yml/badge.svg)](https://github.com/nick-zhang-1991/HyperAgent/actions)
[![Release](https://img.shields.io/badge/release-v0.2.0-blue?style=flat-square)](https://github.com/nick-zhang-1991/HyperAgent/releases/tag/v0.2.0)
[![Rust](https://img.shields.io/badge/rust-1.82+-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)
[![Stars](https://img.shields.io/github/stars/nick-zhang-1991/HyperAgent?style=flat-square)](https://github.com/nick-zhang-1991/HyperAgent/stargazers)
[![i18n](https://img.shields.io/badge/i18n-20_languages-blue?style=flat-square)](https://github.com/nick-zhang-1991/HyperAgent)

> ⚡ Starts <0.3s · 🧠 Cross-project memory · 🐝 Multi-agent parallel · 🌐 20 languages

[English](#) | [中文](README_zh-CN.md) | [日本語](README_ja.md) | [한국어](README_ko.md) | [Español](README_es.md) | [Français](README_fr.md) | [Deutsch](README_de.md) | [Русский](README_ru.md) | [Português](README_pt.md) | [Tiếng Việt](README_vi.md) | [Italiano](README_it.md) | [Türkçe](README_tr.md) | [Polski](README_pl.md) | [العربية](README_ar.md) | [Bahasa Indonesia](README_id.md) | [Українська](README_uk.md) | [Nederlands](README_nl.md) | [ไทย](README_th.md) | [বাংলা](README_bn.md) | [हिन्दी](README_hi.md)

```bash
# Coding
hyper run "implement a thread-safe LRU cache in Rust"

# Analysis
hyper analyze

# Multi-agent
hyper swarm "build REST API + auth + rate limiting"

# Research
hyper run "compare Rust vs Go performance" --mode ask

# DevOps
hyper ci-fix build.log --push

# Self-improvement
hyper feedback good "followed Rust conventions"
```

---

## Installation

```bash
# macOS/Linux
brew install nick-zhang-1991/hyperagent/hyperagent

# or cargo-binstall (fastest)
cargo binstall hyperagent

# or from source
cargo install hyperagent

# or one-liner
curl -fsSL https://raw.githubusercontent.com/nick-zhang-1991/HyperAgent/main/scripts/install.sh | bash
```

---

## What Can It Do?

| Scenario | Example |
|---------|---------|
| 🖥️ Coding | `hyper run "add JWT auth middleware"` |
| 🔍 Analysis | `hyper analyze` — security, complexity, dead code audit |
| 🤖 Automation | `hyper swarm "split monolith into microservices"` |
| 🧪 Testing | `hyper run "write unit tests for UserService"` |
| 📚 Research | `hyper run "compare Rust async vs Go goroutines" --mode ask` |
| 🔧 DevOps | `hyper ci-fix ci.log` |
| 🔒 Security | `hyper run "audit codebase for vulnerabilities"` |
| 📦 Packaging | `hyper run "generate Kubernetes deployment config"` |
| 🌍 Maintenance | Analyze → find bugs → create PR → approve |
| 🧠 Evolution | `hyper feedback good/bad` — teach the agent |

---

## CLI Commands

| Command | What it does |
|---------|-------------|
| `hyper run` | Execute any task with multi-agent pipeline |
| `hyper init` | Onboarding wizard + code index |
| `hyper serve` | Web API server (SSE streaming) |
| `hyper analyze` | Security, complexity, dead code audit |
| `hyper swarm` | Parallel multi-agent execution |
| `hyper ci-fix` | Auto-fix CI pipeline failures |
| `hyper review` | Review staged changes |
| `hyper doctor` | System diagnostics |
| `hyper memory global` | Cross-project knowledge |
| `hyper session share` | Share session via token |
| `hyper feedback` | Teach the agent (RLHF-lite) |
| `hyper skill` | Install/search/create community skills |
| `hyper bench memory` | Memory system benchmark |
| `hyper eval` | Self-evaluation suite |

---

## Unique Selling Points

| Feature | HyperAgent | Claude Code | Aider | Cursor | Devin |
|---------|-----------|------------|-------|--------|-------|
| General-purpose | ✅ | ❌ code only | ❌ code only | ❌ code only | ✅ |
| Multi-agent parallel | ✅ swarm | ❌ | ❌ | ❌ | ✅ |
| Cross-project memory | ✅ | ❌ | ❌ | ❌ | ❌ |
| Self-correction | ✅ feedback | ❌ | ❌ | ❌ | ❌ |
| Deep code analysis | ✅ analyze | ❌ | ❌ | ❌ | ❌ |
| CI auto-fix | ✅ ci-fix | ❌ | ❌ | ❌ | ✅ |
| i18n (20 languages) | ✅ | ❌ | ❌ | ❌ | ❌ |
| Skill marketplace | ✅ | ❌ | ❌ | ❌ | ✅ |
| SSE streaming | ✅ | ✅ | ❌ | ✅ | ✅ |
| VS Code extension | ✅ v0.2 | ❌ | ✅ | ✅ | ✅ |
| Desktop native | ✅ Tauri | ❌ | ❌ | ✅ | ✅ |
| Web UI | ✅ | ❌ | ❌ | ✅ | ✅ |
| Session sharing | ✅ | ❌ | ❌ | ✅ | ✅ |
| Docker sandbox | ✅ | ❌ | ✅ | ❌ | ✅ |

---

## Architecture

```
User Request → CLI · Web · Desktop · VS Code
    ↓
hyper serve (axum + SSE streaming)
    ↓
Agent Pipeline (Plan → Code → Review → Apply → Fix)
    ↓                        ↓                    ↓
Memory System         LLM Provider Pool     Tool System
(14 types, FTS5,      (failover, health,    (MCP, Search,
 embed, auto-prune,    circuit breaker)      Browser, REPL)
 global memory)
    ↓                        ↓                    ↓
Global Memory         Community Skills     Plugin System
```

[Full Mermaid diagrams → docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)

---

## Quick Start

```bash
# Install
brew install nick-zhang-1991/hyperagent/hyperagent

# Configure LLM
export HYPER_LLM_API_KEY=***   export HYPER_LLM_MODEL="gpt-4o"
export HYPER_LLM_BASE_URL="https://api.openai.com/v1"

# Set language (optional)
export HYPER_LANG=zh-CN   # or ja, ko, fr, de, es...

# Initialize
cd your-project && hyper init

# Use it
hyper run "explain this project's architecture"
hyper analyze
hyper serve
```

---

## Community

- [Contributing Guide](CONTRIBUTING.md)
- [Security Policy](SECURITY.md)
- [Code of Conduct](CODE_OF_CONDUCT.md)
- [Changelog](CHANGELOG.md)

### Skills Marketplace

Share your expertise: `hyper skill create` → edit → GitHub Gist → `hyper skill install <gist-url>`

```bash
hyper skill search rust
hyper skill list
```

Browse [skills/](skills/) for seed skills.

---

## Documentation

- [Architecture](docs/ARCHITECTURE.md)
- [Troubleshooting](docs/TROUBLESHOOTING.md)
- [API Reference](docs/API.md)
- [Competitive Analysis](docs/COMPETITIVE_ANALYSIS.md)

---

## License

MIT © 2026 HyperAgent Contributors
