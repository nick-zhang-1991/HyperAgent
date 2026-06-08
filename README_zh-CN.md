# HyperAgent 🚀

**超快 CLI 编码智能体** — 并行多智能体流水线，具备全栈代码理解、自动 LLM 故障转移与生产级安全防护。

[![CI](https://github.com/nick-zhang-1991/HyperAgent/actions/workflows/ci.yml/badge.svg)](https://github.com/nick-zhang-1991/HyperAgent/actions)
[![Release](https://img.shields.io/badge/release-v0.2.0-blue?style=flat-square)](https://github.com/nick-zhang-1991/HyperAgent/releases/tag/v0.2.0)
[![Stars](https://img.shields.io/github/stars/nick-zhang-1991/HyperAgent?style=flat-square)](https://github.com/nick-zhang-1991/HyperAgent/stargazers)

## 安装

```bash
# macOS/Linux (Homebrew)
brew install nick-zhang-1991/hyperagent/hyperagent

# 或 cargo-binstall
cargo binstall hyperagent

# 或源码编译
cargo install hyperagent
```

## 快速开始

```bash
# 设置中文界面
export HYPER_LANG=zh-CN

# 设置 API key
export HYPER_LLM_API_KEY="sk-***"
export HYPER_LLM_BASE_URL="https://api.openai.com/v1"

# 初始化项目
cd your-project
hyper init

# 运行任务
hyper run "给 API 网关加上限流功能"

# 启动 Web 服务
hyper serve

# 启动桌面端
cd gui && pnpm tauri dev
```

## 独家能力

| 能力 | HyperAgent | 其他 CLI Agent |
|------|-----------|---------------|
| 🐝 多 Agent 并行 | ✅ swarm | ❌ 无 |
| 🌍 跨项目全局记忆 | ✅ | ❌ 无 |
| 🧠 自纠错学习 | ✅ feedback | ❌ 无 |
| 📊 深度代码分析 | ✅ analyze | ❌ 无 |
| 🔧 CI 自动修复 | ✅ ci-fix | ❌ 无 |
| 🌐 中文界面 | ✅ | ❌ 无 |
| 📦 Skill 市场 | ✅ | ❌ 无 |

## 全平台

```
CLI 工具     hyper run "任务"
Web 界面     hyper serve → http://127.0.0.1:3000
桌面应用     cd gui && pnpm tauri dev
VS Code      Ctrl+Shift+L 打开聊天面板
```

## 命令参考

| 命令 | 说明 |
|------|------|
| `hyper run "任务"` | 执行编码任务 |
| `hyper init` | 引导向导 + 代码索引 |
| `hyper serve` | 启动 Web API 服务 |
| `hyper analyze` | 深度代码分析 |
| `hyper swarm "任务"` | 多 Agent 并行 |
| `hyper ci-fix log.txt` | CI 自动修复 |
| `hyper review` | 代码审查 |
| `hyper doctor` | 诊断工具 |
| `hyper memory global` | 全局记忆 |
| `hyper session share` | 共享会话 |
| `hyper feedback good/bad` | 训练 Agent |
| `hyper skill install <url>` | 安装技能 |
| `hyper eval` | 自评估 |

## 文档

- [架构图](docs/ARCHITECTURE.md)
- [更新日志](CHANGELOG.md)
- [贡献指南](CONTRIBUTING.md)
- [安全策略](SECURITY.md)
