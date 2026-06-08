# HyperAgent 全面差距分析 & 路线图 (2026-06-08)

## 多维竞品对标矩阵

### 维度 1: 核心 Agent 能力

| 能力 | Claude Code | Codex CLI | Aider | Gemini CLI | Cursor | HyperAgent | 状态 |
|------|------------|----------|-------|-----------|-------|-----------|------|
| 多文件编辑 | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✅ |
| Git 集成 | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✅ |
| 代码索引精度 | grep | grep | repo-map | ✗ | tree-sitter | **tree-sitter+PageRank** | ✅ 领先 |
| Multi-Agent | task tool | plan/exec | ✗ | ✗ | agent | **Plan→Code→Review→Apply** | ✅ 领先 |
| Sub-agent 嵌套 | depth=3 | ✗ | ✗ | ✗ | ✗ | **depth=3 + worktree** | ✅ 领先 |
| 自动编译修复 | ✓ | ✓ | ✓ | ✗ | ✓ | **5轮+去重** | ✅ 新兴 |
| 浏览器自动化 | ✓ | ✗ | ✗ | ✗ | ✗ | ✓ | ✅ |
| MCP 协议 | ✓ | ✗ | ✓ | ✗ | ✓ | **server+client** | ✅ |
| Web 搜索 | ✓ | ✗ | ✗ | ✓ | ✗ | ✓ (DDG) | ✅ |
| Sandbox 沙箱 | Docker | Docker | ✗ | ✗ | ✗ | 字段存在但未实现 | ❌ gap |
| 会话持久化 | ✓ | ✓ | ✓ | ✗ | ✓ | **session save/resume/fork** | ✅ 领先 |
| 记忆系统 | ✓ | ✗ | ✗ | ✗ | ✓ | **14模式+FTS5+embedding** | ✅ 显著领先 |
| 知识库 | ✗ | ✗ | ✗ | ✗ | ✗ | ✓ (统一存储) | ✅ 独特 |
| Token 效率 | ~20k | ~15k | ~8k | ~30k | ~10k | **~3k** | ✅ 显著领先 |
| 启动时间 | ~2s | ~1.5s | ~5s | ~1s | ~1.5s | **~0.3s** | ✅ 显著领先 |
| 打包大小 | ~50MB | ~40MB | ~100MB | ~30MB | ~80MB | **~10MB** | ✅ 显著领先 |
| 语言 | TypeScript | TypeScript | Python | Go | TypeScript | **Rust** | ✅ 独特 |

### 维度 2: 用户体验 & 界面

| 能力 | Claude Code | Codex CLI | Aider | Gemini CLI | Cursor | HyperAgent | 状态 |
|------|------------|----------|-------|-----------|-------|-----------|------|
| CLI 流式输出 | ✓ | ✓ | ✓ | ✓ | - | ✓ | ✅ |
| 彩色 Diff 预览 | ✓ | ✓ | ✓ | ✗ | ✓ | ✓ | ✅ |
| 进度状态 | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ (indicatif) | ✅ |
| 自动 commit | ✓ | ✓ | ✓ | ✗ | ✓ | ✗ | ❌ gap |
| IDE 集成(VS Code) | ✗ | ✗ | ✓ | ✗ | 原生IDE | ✗ | ❌ gap |
| Web UI | ✗ | ✗ | ✗ | ✗ | ✗ | ✓ (Vite+React) | ✅ |
| 桌面应用 | ✗ | ✗ | ✗ | ✗ | ✓ (native) | ✓ (Tauri v2) | ✅ |
| 首次引导 | ✓ | ✓ | ✓ | ✓ | ✓ | ✗ | ❌ gap |
| 终端内 REPL | ✓ | ✓ | ✗ | ✓ | ✓ | ✓ (rustyline) | ✅ |

### 维度 3: 商业化 & 协作

| 能力 | Claude Code | Codex CLI | Aider | Gemini CLI | Cursor | HyperAgent | 状态 |
|------|------------|----------|-------|-----------|-------|-----------|------|
| 多 Provider 故障转移 | ✗ | ✗ | ✓ | ✗ | ✓ | **OnceLock+熔断** | ✅ 领先 |
| 自定义 Mode/System Prompt | ✓ | ✗ | ✓ | ✗ | ✓ | ✓ | ✅ |
| 实时协作/共享 | ✗ | ✗ | ✗ | ✗ | ✓ | ✗ | ❌ gap |
| CI/CD 自动修复 | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ❌ gap |
| PR 审查机器人 | ✗ | ✗ | ✗ | ✗ | GitHub | ✗ | ❌ gap |
| 企业 SSO/RBAC | ✓ | ✗ | ✗ | ✗ | ✓ | ✗ | ❌ gap |
| 使用量统计/计费 | API | API | ✗ | 免费 | $20/月 | ✗ | ❌ gap |
| 团队工作区 | ✗ | ✗ | ✗ | ✗ | ✓ | 基础实现 | 🟡 半 |

### 维度 4: 架构 & 工程

| 能力 | Claude Code | Codex CLI | Aider | Gemini CLI | Cursor | HyperAgent | 状态 |
|------|------------|----------|-------|-----------|-------|-----------|------|
| 代码索引引擎 | grep+扩展 | grep+扩展 | repo-map | ✗ | tree-sitter | **tree-sitter+PageRank** | ✅ 显著领先 |
| Provider Pool | ✗ | ✗ | ✓ | ✗ | ✓ | **health+circuit-breaker** | ✅ 领先 |
| 插件系统 | ✗ | ✗ | ✗ | ✗ | ✗ | ✓ (plugin.rs) | ✅ 独特 |
| 自省/自我评估 | ✓ | ✓ | ✗ | ✗ | ✓ | ✓ (self_reflect) | ✅ |
| 安全工具门控 | ✓ | ✓ | ✗ | ✗ | ✓ | **ToolDangerLevel** | ✅ |
| 构建时间 | ~5s | ~8s | ~3s | ~2s | ~10s | **~2min** (debug) | ⚠️ 改进 |
| 测试覆盖率 | ? | ? | ~200 | ? | ? | **21 memory** | ⚠️ 需加 |
| 错误处理 | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | 🟡 大部分 |
| WASM 支持 | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ❌ gap |

## 差距优先级 (P0-P3)

### P0: 阻塞级 — 必须做 (4 项)

| # | 差距 | 竞品基线 | 影响 | 工作量 |
|---|------|---------|------|--------|
| 1 | **Sandbox Docker 沙箱** | Claude Code, Codex CLI | 安全执行不可靠代码、用户信任、企业级 | M |
| 2 | **自动 Commit** | 所有竞品 | 用户每次要手动 git add/commit，破坏flow | S |
| 3 | **首次引导/Onboarding** | 所有竞品 | 新用户门槛高，无从知晓能力范围 | S |
| 4 | **IDE VS Code 扩展** | Aider, Cursor, Cody | 最大用户获取渠道缺失 | L |

### P1: 高价值 — 尽快做 (5 项)

| # | 差距 | 竞品基线 | 影响 | 工作量 |
|---|------|---------|------|--------|
| 5 | **CI/CD 自动修复 Bot** | 无竞品直接对标 | DevOps 差异化，企业级 | M |
| 6 | **Web UI 生产化** | 无 (独有) | 非 CLI 用户入口 | L |
| 7 | **协作/共享会话** | Cursor, Cody | 团队使用场景 | XL |
| 8 | **构建时间优化** | — | 加速开发迭代 | S |
| 9 | **测试覆盖率提升** | — | 质量保障，~50 测试 → ~200 | M |

### P2: 差异化 — 做更好 (4 项)

| # | 差距 | 竞品基线 | 影响 | 工作量 |
|---|------|---------|------|--------|
| 10 | **使用量仪表盘** | Cursor $20/mo | 商业化基础 | L |
| 11 | **LLM Router 集成** | 无 (llm-router项目) | 智能路由小→大模型 | M |
| 12 | **代码审查 PR Bot** | GitHub Copilot | 开源社区影响力 | M |
| 13 | **Bench 压测 + 可视化** | — | 决策数据 | S |

### P3: 规模化 — 远期 (3 项)

| # | 差距 | 竞品基线 | 工作量 |
|---|------|---------|--------|
| 14 | **企业 SSO/RBAC** | Claude Code, Cursor | XL |
| 15 | **使用量计费系统** | 所有商业竞品 | XL |
| 16 | **WASM 浏览器运行** | 无 | XL |

## 当前独特优势（护城河）

1. **Rust 原生** — 唯一编译型 CLI agent，0.3s 启动，3k tokens/次
2. **Tree-sitter + PageRank** — 代码理解精度最高
3. **Agent 图 + Worktree 隔离** — 竞品无此功能
4. **Provider 故障转移 + 熔断** — 企业级可靠性
5. **插件系统** — 热加载脚本工具
6. **记忆系统** — 14模式+FTS5+embedding+auto-prune
7. **知识库统一存储** — MemoryManager 统一管理
8. **安全门控** — ToolDangerLevel 细粒度
9. **桌面+CLI+Web 三端** — 所有竞品最多两端

## 推荐执行路线

```
Phase 1 (今周) ─── Sandbox → Auto-commit → Onboarding
     │                │          │             │
Phase 2 (本月) ─── VS Code 扩展 → CI Bot → Web UI 生产化
     │                │             │           │
Phase 3 (下月) ─── 协作 → LLM Router → 仪表盘 → Bench
     │
Phase 4 (Q3)  ─── PR Bot → SSO/RBAC → 计费 → WASM
```

开始逐项做。先做 P0 最低工作量：**Auto-commit**（S 级别）。
