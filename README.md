# Coding Agent

一个基于 Rust 的高性能终端编码 Agent，支持 REPL 和 TUI 双模式。

## 特性

- **Agent Loop** — 迭代式多轮工具调用，支持子 Agent（Explore/General）并行执行
- **13 个内置工具** — shell, file_read, file_write, file_edit, grep, glob, ls, git_diff, git_status, git_commit, skill_list, skill_read, web_search
- **TUI 界面** — ratatui 全屏终端（`--tui`），含 Markdown 渲染、语法高亮、工具气泡、审批对话框
- **REPL 界面** — rustyline 命令行，Tab 补全
- **流式输出** — 非阻塞架构，后台 Agent task + channel 事件驱动
- **上下文管理** — token 估算（CJK 优化）、智能压缩、溢出检测
- **记忆系统** — sled 嵌入式 DB，关键词启发式提取，学习闭环
- **会话持久化** — JSONL 存储，会话列表/切换
- **MCP 协议** — stdio/SSE/HTTP + JSON-RPC 2.0
- **Hook 系统** — 9 种事件钩子（PreToolUse, PostToolUse, Stop, SessionStart 等）
- **Shadow Git 快照** — 独立 git-dir，支持 track/diff/restore/revert
- **任务规划器** — brainstorm + plan + decompose + validate
- **技能自进化** — 自动创建 SKILL.md
- **Doom Loop 检测** — 基于 name:arguments_hash，3 次重复自动停止
- **Provider 重试** — 10 次指数退避 + 抖动，区分认证/瞬态错误
- **Tool Output 上限** — 单工具 16KB + 每轮 200KB + 超限强制压缩
- **项目指令** — AGENTS.md 4 级层次搜索

## 快速开始

### 安装

```bash
git clone <repo-url> && cd coding-agent
cargo build --release
```

二进制文件位于 `./target/release/agent`。

### 配置

创建 `~/.agent/config.toml`：

```toml
api_key = "sk-xxx"

[agent]
model = "gpt-4"
temperature = 0.7
max_tokens = 4096
max_iterations = 50
```

或使用环境变量：`OPENAI_API_KEY=sk-xxx`。

### 运行

```bash
# REPL 模式
./target/release/agent

# TUI 模式
./target/release/agent --tui
```

## 命令参考

| 命令 | 说明 |
|------|------|
| `/help` | 显示帮助信息 |
| `/model` | 查看/切换模型 |
| `/skills` | 列出已加载技能 |
| `/clear` | 清空当前上下文 |
| `/quit` | 退出 |
| `/sessions` | 列出所有会话 |
| `/new` | 创建新会话 |
| `/session <id>` | 切换到指定会话 |
| `/mcp` | MCP 服务器管理 |
| `/plan` | 启动任务规划 |
| `/brainstorm` | 头脑风暴 |

## 项目结构

```
coding-agent/
├── crates/
│   ├── agent-cli/       # CLI 入口 + TUI 界面
│   ├── agent-core/      # Agent 引擎、上下文管理、会话、快照
│   ├── agent-tools/     # 13 个内置工具 + MCP 协议
│   ├── agent-llm/       # LLM Provider（OpenAI 兼容）
│   └── agent-memory/    # 记忆系统（sled 嵌入式 DB）
├── Cargo.toml
└── AGENTS.md            # 项目指令文件
```

## 技术栈

| 用途 | 依赖 |
|------|------|
| 异步运行时 | tokio |
| TUI 界面 | ratatui, crossterm |
| Markdown 渲染 | pulldown-cmark, syntect |
| 命令行 REPL | rustyline |
| HTTP 客户端 | reqwest |
| 嵌入式数据库 | sled |
| 序列化 | serde, serde_json |
| 日志 | tracing, tracing-subscriber |

## 架构设计

### Agent 循环

迭代式循环：接收用户输入 → 调用 LLM → 执行工具 → 将结果回传 LLM → 重复直到 LLM 返回最终回答或达到最大迭代次数。支持子 Agent（Explore/General）并行执行。

### 流式输出

非阻塞架构：后台 Agent task 通过 channel 发送事件（文本块、工具调用、状态变更），前端实时渲染，无需等待完整响应。

### 上下文管理

- Token 估算：基于字符数估算，针对 CJK 字符优化
- 智能压缩：上下文接近上限时自动压缩历史消息
- 溢出检测：Tool Output 单工具 16KB / 每轮 200KB 上限，超限强制压缩

### 记忆系统

sled 嵌入式数据库存储，关键词启发式提取。支持学习闭环：nudge 机制触发 LLM 记忆提取，自动积累项目知识。

### Hook 系统

9 种事件钩子，支持自定义脚本执行：PreToolUse, PostToolUse, UserPromptSubmit, Stop, SessionStart, SessionEnd, SubagentStop, Notification, PreCompact。

## 许可证

MIT
