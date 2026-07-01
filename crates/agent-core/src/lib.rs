mod engine;
pub mod context;
pub mod file_diff;
pub mod learning_loop;
pub mod permissions;
pub mod planner;
pub mod project_instructions;
pub mod session;
pub mod skill_evolution;
pub mod snapshot;
pub mod subagent;
pub mod tool_executor;

pub use context::{ContextManager, OverflowLevel, ContextStatus, DEFAULT_MAX_TOKENS, WARNING_THRESHOLD, COMPACTION_BUFFER, PRUNE_MINIMUM, PRUNE_PROTECT, PROTECTED_TOOLS};
pub use engine::AgentEngine;
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

#[derive(Debug, Clone, PartialEq)]
pub enum FinishReason {
    Stop,
    ToolCalls,
    MaxSteps,
    ContextOverflow,
    DoomLoop,
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum LoopState {
    Idle,
    Running,
    WaitingTool,
    Completed,
    Failed(String),
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct AgentConfig {
    pub system_prompt: String,
    pub model: String,
    pub temperature: f64,
    pub max_tokens: u32,
    pub max_iterations: usize,
    #[serde(default = "default_working_dir")]
    pub working_dir: String,
    #[serde(default = "default_permission_mode")]
    pub permission_mode: String,
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
