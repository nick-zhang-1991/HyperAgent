# HyperAgent 竞争力分析 (2026-07) — 更新版

## 核心能力对比

| 能力 | HyperAgent | Hermes Agent | Aider | Claude Code | Cline |
|------|-----------|-------------|-------|-------------|-------|
| **并行智能体** | ✅ N-way tokio | ❌ 单线程 | ❌ 顺序 | ❌ 顺序 | ❌ 顺序 |
| **持久化记忆** | ✅ SQLite + 向量嵌入 | ✅ 每轮注入 | ❌ 无 | ✅ 项目记忆 | ✅ 会话记忆 |
| **技能系统** | ✅ ~/.hyper/skills/ + import/export/search/sync | ✅ 完整技能系统 | ❌ 无 | ❌ 无 | ❌ 无 |
| **MCP 支持** | ✅ Stdio+HTTP | ✅ 原生 MCP | ❌ | ✅ 内置 | ✅ MCP 客户端 |
| **浏览器自动化** | ✅ CDP WebSocket | ✅ computer_use | ❌ | ❌ | ❌ |
| **后台进程** | ✅ /bg 命令管理 | ❌ 需 skill | ❌ | ❌ | ❌ |
| **GUI操作电脑** | ✅ macOS: screenshot, mouse, keyboard, osascript, apps | ✅ 原生 (截图+点击+键盘) | ❌ | ❌ | ❌ |
| **任务看板** | ✅ Kanban Board + Web UI | ❌ 无 | ✅ Architect模式 | ❌ | ✅ 多代理 |
| **API 路由** | ✅ Provider Pool + 加权路由 + 冷却 | ✅ 自定义 Provider | ✅ 多 API | ❌ | ✅ |
| **Streaming UX** | ✅ 实时进度 (文件名/字符数/计时) | ✅ 实时流式 | ✅ | ✅ | ✅ |
| **Lint 自动修复** | ✅ 最多3轮 cargo check/tsc | ✅ 技能驱动 | ✅ Auto fix | ❌ | ❌ |
| **SWE-bench 评估** | ✅ 内置 7 任务 | ❌ | ✅ SWE-bench | ❌ | ❌ |
| **远程节点** | ✅ SSH + SCP + TCP 服务器 | ✅ Tailscale + SSH | ❌ | ❌ | ❌ |
| **Cron 任务** | ✅ 定时调度 | ✅ Cron 系统 | ❌ | ❌ | ❌ |
| **生命周期钩子** | ✅ 12 种事件 | ✅ Hooks | ❌ | ❌ | ❌ |
| **代码索引** | ✅ PageRank 4 策略 | ✅ CodeGraph | ✅ Tree-sitter | ✅ 内置 | ✅ |
| **Web 仪表盘** | ✅ memory + skills + kanban | ✅ Hermes Desktop | ❌ | ❌ | ❌ |
| **多模态视觉** | ✅ analyze_image + analyze_screenshot | ✅ vision_analyze | ✅ 图片 | ❌ | ✅ 图片 |
| **基准测试** | ✅ hyper benchmark (7 任务) | ❌ | ✅ SWE-bench | ❌ | ❌ |
| **会话管理** | ✅ fork/merge/tree/branch | ✅ 会话树 | ❌ | ❌ | ❌ |
| **安全沙箱** | ✅ 13 种危险模式检测 | ✅ 工具审批 | ⚠️ 基础 | ✅ | ✅ |
| **自动变更日志** | ✅ conventional commits | ❌ | ❌ | ❌ | ❌ |
| **Shell 补全** | ✅ 5 种 shell | ❌ | ❌ | ✅ | ❌ |
| **配置热加载** | ✅ mtime 监控 | ✅ 配置 CLI | ❌ | ❌ | ❌ |
| **增量索引** | ✅ 文件监听器 | ✅ 自动 | ❌ | ❌ | ❌ |
| **跨平台发布** | ✅ CI/CD (5 targets) | ❌ | ✅ PyPI | ✅ npm | ✅ VSIX |
| **TUI 仪表盘** | ✅ ratatui | ❌ | ❌ | ❌ | ❌ |

## 关键差异分析

### HyperAgent 领先于 Hermes Agent 的领域

| 领域 | 说明 |
|------|------|
| **并行智能体** | HyperAgent 支持 N 路并行 CodeAgent，Hermes 单线程 |
| **后台进程管理** | HyperAgent 有完整的 /bg 子系统 |
| **任务看板** | HyperAgent 内置 Kanban + Web UI |
| **SWE-bench 评估** | HyperAgent 有内置评估框架 |
| **基准测试套件** | HyperAgent 有 7 道编程挑战自动评测 |
| **会话管理** | HyperAgent 支持 fork/merge/tree |
| **自动变更日志** | HyperAgent 支持 conventional commits |
| **Shell 补全** | HyperAgent 支持 5 种 shell |
| **TUI 仪表盘** | HyperAgent 有终端仪表盘 (ratatui) |
| **跨平台发布** | HyperAgent 有 CI/CD 自动构建 5 平台 |

### Hermes Agent 领先于 HyperAgent 的领域

| 领域 | 说明 |
|------|------|
| **记忆系统精细度** | Hermes 区分 user.md + memory.md，每轮注入 |
| **Herkes Desktop** | Hermes 有原生 macOS 桌面应用 (arm64) |
| **MCP 生态** | Hermes MCP 服务器支持更丰富 |
| **代码索引深度** | Hermes 的 CodeGraph 使用 tree-sitter AST |
| **native MCP 客户端** | Hermes 内置 MCP 客户端发现配置中的服务器 |
| **agent 工具集** | Hermes 有 skill_view/skill_manage/读写文件等更多原生工具 |

### 持平领域

| 领域 | 说明 |
|------|------|
| **技能系统** | 功能接近，各有特色 |
| **GUI 操作电脑** | 都支持 macOS osascript，HyperAgent 新增 |
| **远程节点** | 都支持 SSH 远程，HyperAgent 新增 |
| **Web 面板** | 都支持，HyperAgent 新增 |
| **多模态** | 都支持 vision API，HyperAgent 新增 |

## 10/10 待办清单

以下是从 9.2 → 10.0 需要补齐的能力：

### P0 — 必须 (直接用户可见)

1. **跨会话对话搜索** — `hyper session search <query>` 搜索历史对话内容
2. **Memory 前端编辑** — dashboard 中支持编辑/更新记忆内容，不只是查看/删除
3. **配置 Web 面板** — dashboard 增加 `/api/config` 端点，可在浏览器中修改配置
4. **技能版本管理** — 技能文件带版本号，支持 diff/rollback

### P1 — 重要 (显著提升体验)

5. **Windows/Linux computer_use** — 使用 AutoIt (Windows) 或 xdotool (Linux) 扩展桌面操作
6. **MCP 服务端市场** — `hyper skills import` 支持从 GitHub 仓库批量导入技能
7. **多轮自动修复** — 当 cargo check 失败时，自动重试修复直到通过
8. **Agent 自我优化** — agent 分析自己的表现报告，自动调参

### P2 — 锦上添花

9. **多 Agent 协作模式** — 多个 agent 实例分工协作同一任务
10. **性能仪表盘** — benchmark 结果可视化，历史趋势图
11. **代码审查自动 PR** — 自动创建 GitHub PR 并添加 review comments
12. **插件市场** — `hyper plugin install <name>` 从社区仓库安装插件

### P3 — 长期愿景

13. **Hermes Desktop 风格原生应用** — macOS SwiftUI 桌面应用
14. **ComfyUI 集成** — 视频/音频生成工作流
15. **多仓库协作** — 跨多个 GitHub 仓库的原子性改动
16. **分布式 agent 集群** — 多台机器协作完成大型重构

## 评分当前状态

```
总分: 9.2/10

各维度评分:
  🏗️ 架构:       10/10 (并行流水线, MCP, 插件, hooks)
  🧠 记忆:       8/10 (SQLite 存储, 无前端编辑)
  🎨 UI/UX:      9/10 (REPL, TUI, Web Dashboard)
  🔌 集成:       9/10 (MCP, SSH, vision, browser)
  🛡️ 安全:       10/10 (13模式检测, 路径保护)
  ⚡ 性能:       9/10 (增量索引, 上下文压缩)
  🧪 测试:       8/10 (167单元测试, 基准测试)
  📦 发布:       10/10 (CI/CD, 5平台)
  📚 文档:       8/10 (双语 README, competitive analysis)
  🌐 生态:       9/10 (技能社区, import/export/sync)
```
