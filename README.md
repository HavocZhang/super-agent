# Coding Agent

一个基于 Rust 的高性能编码 Agent，参考了 Codex CLI、Pi Agent、magic-code 等主流 Agent 的架构设计。

## 特性

- **Agent Loop**: 类似 Codex 的 agentic loop，支持多轮工具调用
- **内置工具**: shell, file_read, file_write, file_edit, grep, glob, ls
- **LLM 路由**: 支持 OpenAI 兼容接口 (OpenAI, DeepSeek, Ollama 等)
- **记忆系统**: 基于 sled 的本地记忆存储
- **配置驱动**: TOML 配置文件 + 环境变量

## 快速开始

### 1. 配置

创建 `~/.agent/config.toml`:

```toml
api_key = "sk-xxx"

[agent]
system_prompt = "You are a powerful coding agent."
model = "gpt-4"
temperature = 0.7
max_tokens = 4096
max_iterations = 50
```

### 2. 运行

```bash
# 使用配置文件
cargo run --release

# 或使用环境变量
OPENAI_API_KEY=sk-xxx cargo run --release
```

### 3. 使用

```
🤖 Agent: 你好！我是你的编码助手，有什么可以帮你的？

👤 你: 帮我用 Go 实现一个 Hello World

🤖 Agent: 我来帮你创建...
[调用工具: file_write]
[结果: Successfully written to main.go]
```

## 项目结构

```
coding-agent/
├── crates/
│   ├── agent-cli/          # CLI 入口
│   ├── agent-core/         # Agent 运行时
│   ├── agent-tools/        # 工具系统
│   ├── agent-llm/          # LLM Provider
│   └── agent-memory/       # 记忆系统
└── Cargo.toml
```

## 技术栈

- **Rust**: 高性能、内存安全
- **tokio**: 异步运行时
- **reqwest**: HTTP 客户端
- **sled**: 嵌入式数据库

## 许可证

MIT
