# Coding Agent 项目指南

## 项目信息
- **语言**: Rust（禁止 Python/JavaScript/其他语言）
- **仓库**: /home/havoc/workspace/coding-agent/
- **架构**: Cargo workspace，5 个 crate

## Crate 结构
| Crate | 路径 | 功能 |
|-------|------|------|
| agent-cli | crates/agent-cli/ | CLI 入口 + TUI 界面 |
| agent-core | crates/agent-core/ | Agent 引擎、上下文管理、会话、快照 |
| agent-tools | crates/agent-tools/ | 13 个内置工具 + MCP |
| agent-llm | crates/agent-llm/ | LLM Provider（OpenAI 兼容） |
| agent-memory | crates/agent-memory/ | 记忆系统（sled 嵌入式 DB） |

## 开发规则
1. **只写 Rust**，使用 `///` 文档注释和 `//` 行内注释
2. **README.md 用中文**编写
3. **commit message 用中文**
4. 每次修改后必须 `cargo build --release` 验证编译
5. 每次修改后必须 `cargo test` 验证测试通过
6. 遵循现有代码风格，不要引入新抽象
7. 不要添加不必要的注释

## 常用命令
```bash
cargo build --release          # 编译
cargo test                     # 运行测试
./target/release/agent         # REPL 模式
./target/release/agent --tui   # TUI 模式
```

## 配置文件
- Agent 配置: ~/.agent/config.toml
- 记忆数据库: ~/.agent/memory.db
- 会话存储: ~/.agent/sessions/
