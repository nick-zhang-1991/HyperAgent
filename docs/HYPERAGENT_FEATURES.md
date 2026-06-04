# HyperAgent — 功能与设计总览

> 从所有顶级 AI Agent 项目中博采众长，以 Rust 实现的最快 CLI 编码 Agent

## 一、已分析的开源项目（>20K★）

### 核心分析项目（已深入读源码）
| 项目 | Stars | 语言 | 启发要点 |
|------|-------|------|----------|
| **Aider** | ~35K | Python | Map-reduce编辑模型、多LLM支持、lint-driven修正、benchmark框架 |
| **OpenAI Codex CLI** | ~85K | Python | Session管理、Artifact机制、Sub-agent拓扑、工作流钩子 |
| **Cline** | ~62K | TypeScript | Kanban任务板、工作树隔离、多Agent编排、文件编辑协议 |
| **mem0** | ~56K | Python | 智能记忆（实体提取、图关联、重要性评分、时间衰减） |
| **OpenHands** | ~74K | Python | 浏览器沙箱、事件流架构、安全执行策略 |
| **Goose** | ~45K | Rust | MCP协议支持、Shell-based交互、分层工具系统 |
| **Roo-Code** | ~24K | TypeScript | Agent模式（Code/Architect/Ask/Debug）、角色prompt模板、细粒度权限 |
| **SWE-agent** | ~16K | Python | Agent-Computer Interface（ACI）、系统命令、配置驱动的反馈 |

### 辅助参考项目（awesome-ai-agents 中相关）
| 项目 | Stars | 设计要点 |
|------|-------|----------|
| **MetaGPT** | ~45K | 角色模拟（PM/架构/工程师/QA），软件公司工作流 |
| **AutoGen** | ~38K | 多Agent对话框架，代理拓扑、终止条件、会话模式 |
| **CrewAI** | ~25K | 角色+任务+流程编排、层级/顺序/协商流程 |
| **ChatDev** | ~26K | 角色扮演式多Agent协作，链式思考 |
| **CAMEL** | ~22K | 角色扮演启发式，任务指定Agent+任务执行Agent |
| **GPT Researcher** | ~15K | 并行研究Agent，深度+广度搜索 |
| **GPT Pilot** (archived) | ~34K | 渐进式代码生成，调试-修复循环 |

---

## 二、功能矩阵：HyperAgent vs 其他项目

| 功能特性 | HyperAgent | Aider | Codex | Cline | Roo-Code | OpenHands |
|----------|:----------:|:-----:|:-----:|:-----:|:--------:|:---------:|
| **语言** | Rust ✅ | Python | Python | TS | TS | Python |
| **编译速度** | 纳秒级(native) ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| **流式输出** | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **多LLM Provider** | ✅ 深+OpenAI+火山 | ✅ | ❌ | ✅ | ✅ | ✅ |
| **代理故障转移** | ✅ 优先级+权重 | ✅ | ❌ | ❌ | ❌ | ❌ |
| **PageRank代码索引** | ✅ 独创 | ❌ | ❌ | ❌ | ❌ | ❌ |
| **tree-sitter解析** | ✅ 独创 | ❌ | ❌ | ❌ | ❌ | ❌ |
| **SQLite缓存** | ✅ 独创 | ❌ | ❌ | ❌ | ❌ | ❌ |
| **智能记忆系统** | ✅ | ✅ 偏平 | ✅ | ❌ | ❌ | ❌ |
| **实体提取+链接** | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| **重要性+时间评分** | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| **多Agent并行** | ✅ | ❌ | ✅ | ✅ | ✅ | ✅ |
| **Kanban任务板** | ✅ | ❌ | ❌ | ✅ | ✅ | ❌ |
| **依赖链** | ✅ | ❌ | ❌ | ✅ | ✅ | ❌ |
| **工作树隔离** | ✅ | ❌ | ✅ | ✅ | ✅ | ❌ |
| **Agent模式** | ✅ | ❌ | ❌ | ❌ | ✅ | ❌ |
| **逐工具权限** | ✅ | ❌ | ❌ | ❌ | ✅ | ❌ |
| **MCP集成** | ✅ | ❌ | ❌ | ✅ | ✅ | ❌ |
| **浏览器自动化(CDP)** | ✅ | ❌ | ❌ | ❌ | ✅(VS Code) | ✅ |
| **后台进程管理** | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| **生命周期钩子** | ✅ | ❌ | ✅ | ❌ | ❌ | ❌ |
| **Agent拓扑图** | ✅ | ❌ | ✅ | ❌ | ❌ | ❌ |
| **Session管理** | ✅ | ❌ | ✅ | ❌ | ❌ | ✅ |
| **AGENTS.md** | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| **.hyperignore** | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| **热键/键盘模式** | ❌ | ❌ | ❌ | ✅ | ✅ | ❌ |
| **API Router多模型** | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| **发布构建** | ✅(已修复) | ✅ | ✅ | ✅ | ✅ | ✅ |
| **TUI交互** | ❌(未启用) | ❌ | ❌ | ✅ | ✅ | ❌ |
| **文件监视** | ❌(未启用) | ✅ | ❌ | ❌ | ❌ | ❌ |

---

## 三、设计借鉴清单

### 从 Aider 借鉴
1. **Map-Reduce 编辑模型** — 先 map 找出所有需要修改的位置，再 reduce 应用修改
2. **lint-driven 修正** — 生成代码后自动运行 linter，根据 lint 错误修正
3. **多 LLM 支持** — 可配置的 provider + model 组合

### 从 OpenAI Codex CLI 借鉴
1. **Session 管理** — "超轻量级 checkpoint"，JSON 持久化会话，可 resume
2. **Sub-agent 拓扑** — Agent Graph 有向图：父级 spawn 子级，追踪任务生命周期
3. **Artifact 机制** — 每个 session 有输入/输出日志
4. **生命周期钩子** — pre_run, post_code, on_complete 等 12 事件
5. **工作流钩子** — 支持 shell 命令、脚本、webhook 三种 action

### 从 Cline 借鉴
1. **Kanban 任务板** — 卡片管理并行任务，依赖链执行
2. **工作树隔离** — 每个 agent 在独立的 git worktree 中工作
3. **文件编辑协议** — 结构化的文件变更格式

### 从 mem0 借鉴
1. **智能记忆** — SQLite 存储、实体提取、多信号检索
2. **重要性评分** — 基于关键词（bug/critical/fix 等）自动打分
3. **时间衰减排序** — 最近访问的记忆获得更高权重
4. **实体链接** — 跨记忆的实体关联，提高检索 recall

### 从 Roo-Code 借鉴
1. **Agent 模式** — Code/Architect/Ask/Debug/Custom 五种模式
   - Code: 全部权限，生产编码
   - Architect: 只读 + git，只出方案不动代码
   - Ask: 只读，不执行任何命令
   - Debug: 读文件 + 执行命令，不修改
2. **逐工具权限矩阵** — 每个模式对 edit/bash/read/network 有独立的 Allowed/ReadOnly/Denied 等级
3. **角色 prompt 模板** — 每个模式有专门的 system prompt

### 从 OpenHands 借鉴
1. **事件流架构** — 序列化的事件日志，可回放/调试
2. **安全执行策略** — 命令白名单/黑名单

### 从 Goose 借鉴
1. **MCP 协议支持** — 通过 MCP 集成外部工具和服务器
2. **分层工具系统** — 工具按功能分层，不同模式暴露不同层级

### 从 SWE-agent 借鉴
1. **ACI 设计** — Agent-Computer Interface 的精心设计
2. **配置驱动的反馈系统** — 根据命令输出自动调整 agent 行为

### 从 MetaGPT 借鉴（未来方向）
1. **角色模拟** — PM/架构师/工程师/QA 分角色协作
2. **软件公司工作流** — 文档→设计→开发→测试流水线

### 从 AutoGen/CrewAI 借鉴（未来方向）
1. **多 Agent 对话** — 管理 agent 间的通信拓扑
2. **终止条件** — 基于条件的自动停止

---

## 四、HyperAgent 独特创新

### 1. PageRank 代码索引（独创）
- 使用 PageRank 算法计算代码文件中符号（函数、类、结构体）的重要性
- 参考图构建：符号间的引用关系形成有向图
- 查询时仅发送最重要的文件，极大降低 token 消耗

### 2. tree-sitter 解析（独创）
- 支持 20+ 编程语言的精确 AST 解析
- 提取函数/类/结构体/枚举等符号及其行号
- 跨文件引用检测（目前基本，可增强）

### 3. SQLite 缓存持久化（独创）
- 索引结果缓存到 SQLite，后续秒级加载
- 支持 .hyperignore 配置忽略文件

### 4. Rust 原生性能（独有优势）
- 编译到 native binary，启动零延迟
- tokio 异步运行时，所有调用并发
- 内存安全，无 GC 停顿

### 5. Pipeline 端到端集成（独特）
- 唯一将 记忆→索引→规划→并行编码→评审→应用→记录 全链路打通的项目
- 记忆自动注入 prompt，减少 agent 重复犯错
- 智能工作分割：按步骤/文件关联度分配任务

---

## 五、已配置的 LLM Provider

| Provider | Base URL | 默认模型 | 优先级 |
|----------|----------|----------|--------|
| **Volcengine** (中国可用) | `ark.cn-beijing.volces.com/api/coding/v3` | deepseek-v4-flash | 1 |
| **DeepSeek** (备用) | `api.deepseek.com/v1` | deepseek-v4-flash | 2 |
| **OpenAI** (备用) | `api.openai.com/v1` | gpt-4o | 3 |

---

## 六、命令行速查

```bash
hyper run "修复bug" --mode debug --agents 3         # 运行编码任务
hyper init                                            # 构建索引
hyper doctor                                          # 诊断
hyper mode list                                       # 列出模式
hyper memory list                                     # 查看记忆
hyper memory search "bug fix"                         # 搜索记忆
hyper kanban add "重构" "拆模块X" --mode code          # 添加看板卡片
hyper kanban start                                    # 并行执行所有就绪卡片
hyper kanban board                                    # 看板概览
hyper hooks list                                      # 查看钩子
hyper session list                                    # 查看会话
hyper graph tree                                      # 查看Agent拓扑
```

## 七、已实现的增强

| 增强项 | 状态 | 说明 |
|--------|:----:|------|
| **Release 构建** | ✅ | `CARGO_PROFILE_RELEASE_LTO=off cargo build --release` |
| **编译警告清理** | ✅ | 50→43 (cargo fix)，持续减少 |
| **`--reindex` 别名** | ✅ | `hyper init --reindex` = `hyper init --force` |
| **ModelRouter CLI集成** | ✅ | `hyper run` 优先读取 `config.toml` 选 provider |
| **一键安装脚本** | ✅ | `scripts/install.sh` |
| **ASK 模式直出** | ✅ | 跳过计划/编码流水线，直接 Q&A |
| **Web Search** | ❌ | 待开发 |
| **TUI 交互** | ❌ | 已存在代码但 feature-gate 关闭 |
| **增量文件监视** | ❌ | `watcher.rs` 已存在但未启用 |
| **跨文件引用检测** | ❌ | Parser 返回 0 引用 |

---

## 八、下一步可增强方向

参考 awesome-ai-agents 中的新发现：

1. **GPT Pilot 的调试-修复循环** — 渐进式代码生成，自动检测并修复错误
2. **MetaGPT 的角色协作** — PM/架构师/工程师/QA 分角色工作流
3. **AutoGen 对话管理** — 更灵活的 agent 间通信拓扑
4. **CrewAI 的流程编排** — 顺序/层级/协商三种模式
5. **ChatDev 的角色扮演** — 让 agent 在特定角色上下文中思考
6. **GPT Researcher 的搜索策略** — 深度+广度双层搜索，适合研究任务
7. **SWE-agent 的 ACI 设计** — 更精巧的命令行接口
8. **Rust 发布编译** — 启用 release build（LTO 优化），开启 TUI 功能
9. **增量文件监视** — `watcher.rs` 已存在但未启用
10. **跨文件引用检测** — 目前 parser 返回 0 引用，需增强
11. **Web 搜索** — 添加 `web_search` 工具用于研究型任务
12. **Git 差异分析和自动提交** — 增强 diff 分析能力
