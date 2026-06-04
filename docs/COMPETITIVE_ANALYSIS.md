# HyperAgent 竞争力分析 (2026-07)

## 核心能力对比

| 能力 | HyperAgent | Hermes Agent | Aider | Claude Code | Cline |
|------|-----------|-------------|-------|-------------|-------|
| **记忆系统** | ✅ SQLite + 向量嵌入(可选) | ✅ 持久化 Memory (注入每轮) | ❌ 无 | ✅ 项目记忆 | ✅ 会话记忆 |
| **技能系统** | ✅ ~/.hyper/skills/ | ✅ 完整技能系统 | ❌ 无 | ❌ 无内置 | ❌ 无 |
| **MCP 支持** | ✅ Stdio+HTTP | ✅ 原生 MCP | ❌ | ✅ 内置 | ✅ MCP 客户端 |
| **浏览器自动化** | ✅ CDP + WebSocket | ✅ computer_use | ❌ | ❌ | ❌ |
| **后台进程** | ✅ /bg 命令管理 | ❌ 需 skill | ❌ | ❌ | ❌ |
| **GUI操作电脑** | ❌ 无 | ✅ 原生 (截图+点击+键盘) | ❌ | ❌ | ❌ |
| **任务看板** | ✅ Kanban Board | ❌ 无 | ✅ Architect模式 | ❌ | ✅ 多代理 |
| **API 路由** | ✅ Provider Pool + 加权路由 | ✅ 自定义 Provider | ✅ 多 API | ❌ | ✅ |
| **Streaming UX** | ✅ 实时进度 | ✅ 实时流式 | ✅ | ✅ | ✅ |
| **Lint 自动修复** | ✅ 最多3轮 | ✅ 技能驱动 | ✅ Auto fix | ❌ | ❌ |
| **E2E 评估** | ✅ SWE-bench + 内置测试 | ❌ | ✅ SWE-bench | ❌ | ❌ |
| **多节点部署** | ❌ 无 | ✅ 远程节点 | ❌ | ❌ | ❌ |
| **Cron 任务** | ✅ 定时调度 | ✅ Cron 系统 | ❌ | ❌ | ❌ |
| **生命周期钩子** | ✅ Hooks | ✅ Hooks | ❌ | ❌ | ❌ |
| **代码索引** | ✅ PageRank | ✅ CodeGraph | ✅ Tree-sitter | ✅ 内置 | ✅ |

## 关键差距分析

### 1. 记忆系统 — Hermes Agent 领先

**Hermes Agent**: 持久性 `memory.md` + `user.md` 每轮注入，支持预定义知识、系统配置、项目环境、操作记录。前端面板可查看/编辑记忆。用户角色 profile 区分记忆和用户偏好。

**HyperAgent**: SQLite 存储记忆，支持向量嵌入(通过 Ollama)做语义检索。更新/归档/合并/提取等操作。但不如 Hermes 精细：无前端面板，无用户/系统记忆分离，无技能间跳转。

### 2. 操作电脑能力 — Hermes Agent 显著领先

**Hermes Agent**: 原生 `computer_use` 工具集，`osascript` 控制 macOS GUI (微信等桌面应用)，截图+点击+键盘操作，桌面远程控制 (Tailscale/SSH)。Hermes Desktop 官方应用 (arm64)。

**HyperAgent**: CDP 浏览器自动化（Chrome devtools protocol），可截屏、点击、获取源码。但**不支持桌面GUI操作**，无法控制浏览器外的应用。

### 3. 技能系统 — 各有特色

**Hermes Agent**: 自动匹配技能描述，`skill_view()`/`skill_manage()` 工具管理，自动归档/更新过时技能。YAML frontmatter，文件丰富。

**HyperAgent (新实现)**: 类似架构但更简单：~/.hyper/skills/ 存储，SKILL.md 格式，CLI 命令管理，自动保存复杂任务，自动加载匹配技能到上下文。

### 4. 远程部署 — Hermes Agent 领先

**Hermes Agent**: 多节点 SSH 远程维护，Tailscale 组网，远程隧道，Docker Compose 多服务器编排。

**HyperAgent**: 无远程部署能力。

### 5. 多模态 — 平手 (都不足)

两者都需要 MCP 来实现图片/视频处理。HyperAgent 支持图片输入到支持 vision 的模型，Hermes 支持 vision_analyze。

## 优先改进方向

1. **GUI操作电脑**: 实现 `computer_use` 工具集（截图→点击→键盘），用 osascript/Swift 或 Python 实现跨平台
2. **前端记忆面板**: Web 界面查看/编辑/管理记忆和技能
3. **远程节点管理**: SSH 远程目标机连接，类似 Hermes 的远程 agent 访问
4. **多模态强化**: 图片理解、视频帧分析、ComfyUI 集成
5. **技能社区**: 共享技能仓库、自动技能发现
