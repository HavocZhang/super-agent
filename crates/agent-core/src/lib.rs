// ── agent-core 核心库 ──────────────────────────────────────
// Agent 引擎、上下文管理、文件差异、权限、规划器、记忆等模块

mod engine;
pub mod context;
pub mod file_diff;
pub mod hooks;
pub mod learning_loop;
pub mod permissions;
pub mod planner;
pub mod project_instructions;
pub mod session;
pub mod skill_evolution;
pub mod snapshot;
pub mod subagent;
pub mod tool_executor;

// 重新导出核心类型
pub use context::{ContextManager, OverflowLevel, ContextStatus, DEFAULT_MAX_TOKENS, WARNING_THRESHOLD, COMPACTION_BUFFER, PRUNE_MINIMUM, PRUNE_PROTECT, PROTECTED_TOOLS};
pub use engine::{AgentEngine, truncate_tool_output};
pub use file_diff::FileDiff;
pub use snapshot::{SnapshotFileDiff, SnapshotManager, SnapshotPatch, SnapshotDiffStatus};
pub use learning_loop::LearningLoop;
pub use permissions::{PermissionManager, PermissionMode, PermissionResult};
pub use planner::TaskPlanner;
pub use project_instructions::ProjectInstructions;
pub use session::{Session, SessionMessage, SessionStore};
pub use skill_evolution::{Skill, SkillEvolution};
pub use subagent::{SubagentManager, SubagentTask, SubagentType, SubagentResult};
pub use tool_executor::{ToolExecutor, ToolResult};
pub use hooks::{HookEvent, HookConfig, HookResult, HookExecutor};

/// Agent 循环结束的原因
#[derive(Debug, Clone, PartialEq)]
pub enum FinishReason {
    /// 正常停止，LLM 返回文本响应
    Stop,
    /// LLM 请求调用工具
    ToolCalls,
    /// 达到最大步骤限制
    MaxSteps,
    /// 上下文窗口溢出
    ContextOverflow,
    /// 检测到死循环（同一工具连续调用多次）
    DoomLoop,
    /// 其他错误
    Error(String),
}

/// Agent 循环的当前状态
#[derive(Debug, Clone, PartialEq)]
pub enum LoopState {
    /// 空闲状态
    Idle,
    /// 运行中
    Running,
    /// 等待工具执行结果
    WaitingTool,
    /// 已完成
    Completed,
    /// 失败，包含错误信息
    Failed(String),
}

/// Agent 配置信息
#[derive(Debug, Clone, serde::Deserialize)]
pub struct AgentConfig {
    /// 系统提示词，定义 agent 的行为
    pub system_prompt: String,
    /// LLM 模型名称 (如 "gpt-4", "deepseek-chat")
    pub model: String,
    /// 温度参数 (0.0~1.0)
    pub temperature: f64,
    /// 每次 LLM 调用的最大 token 数
    pub max_tokens: u32,
    /// Agent 循环的最大迭代次数
    pub max_iterations: usize,
    /// 工作目录
    #[serde(default = "default_working_dir")]
    pub working_dir: String,
    /// 权限模式 (default/auto/plan/yolo)
    #[serde(default = "default_permission_mode")]
    pub permission_mode: String,
    /// 上下文窗口最大 token 数
    #[serde(default = "default_context_max_tokens")]
    pub context_max_tokens: usize,
}

fn default_working_dir() -> String {
    ".".to_string()
}

fn default_permission_mode() -> String {
    "default".to_string()
}

fn default_context_max_tokens() -> usize {
    100_000
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            system_prompt: "You are a powerful coding agent. You can read and write files, execute shell commands, and search code. Always explain what you're doing before using tools.".to_string(),
            model: "gpt-4".to_string(),
            temperature: 0.7,
            max_tokens: 4096,
            max_iterations: 100,
            working_dir: ".".to_string(),
            permission_mode: "default".to_string(),
            context_max_tokens: 100_000,
        }
    }
}
