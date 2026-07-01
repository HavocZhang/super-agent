mod file_edit;
mod file_read;
mod file_write;
mod git_commit;
mod git_diff;
mod git_status;
mod glob_tool;
mod grep;
mod ls;
pub mod mcp_bridge;
pub mod mcp_client;
pub mod mcp_manager;
mod registry;
mod shell;
mod skill_list;
mod skill_read;
mod web_search;

pub use mcp_manager::McpManager;
pub use registry::ToolRegistry;

use async_trait::async_trait;
use serde_json::Value;

// ── 工具系统 ────────────────────────────────────────────────
// 定义 Tool trait 和默认工具集合

/// 工具 trait —— 所有工具必须实现此接口
#[async_trait]
pub trait Tool: Send + Sync {
    /// 工具名称（如 "file_read", "shell"）
    fn name(&self) -> &str;
    /// 工具描述（用于 LLM 了解工具用途）
    fn description(&self) -> &str;
    /// 工具输入参数的 JSON Schema
    fn input_schema(&self) -> Value;
    /// 执行工具，返回输出文本
    async fn execute(&self, args: &Value, working_dir: &str) -> anyhow::Result<String>;
}

/// 创建默认工具集（所有工具）
pub fn default_tools() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(shell::ShellTool));
    registry.register(Box::new(file_read::FileReadTool));
    registry.register(Box::new(file_write::FileWriteTool));
    registry.register(Box::new(file_edit::FileEditTool));
    registry.register(Box::new(grep::GrepTool));
    registry.register(Box::new(glob_tool::GlobTool));
    registry.register(Box::new(ls::LsTool));
    registry.register(Box::new(git_diff::GitDiffTool));
    registry.register(Box::new(git_status::GitStatusTool));
    registry.register(Box::new(git_commit::GitCommitTool));
    registry.register(Box::new(skill_list::SkillListTool));
    registry.register(Box::new(skill_read::SkillReadTool));
    registry.register(Box::new(web_search::WebSearchTool));
    registry
}

pub fn readonly_tools() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(file_read::FileReadTool));
    registry.register(Box::new(grep::GrepTool));
    registry.register(Box::new(glob_tool::GlobTool));
    registry.register(Box::new(ls::LsTool));
    registry.register(Box::new(git_diff::GitDiffTool));
    registry.register(Box::new(git_status::GitStatusTool));
    registry.register(Box::new(skill_list::SkillListTool));
    registry.register(Box::new(skill_read::SkillReadTool));
    registry
}

pub fn register_custom_tools(registry: &mut ToolRegistry, skills_dir: &str) {
    use std::path::Path;

    let dir = Path::new(skills_dir);
    if !dir.exists() {
        return;
    }

    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let skill_file = entry.path().join("SKILL.md");
        if skill_file.exists() {
            if let Ok(content) = std::fs::read_to_string(&skill_file) {
                if let Some(tool) = CustomSkillTool::from_skill_md(&name, &content) {
                    tracing::info!("Registered custom skill tool: {}", name);
                    registry.register(Box::new(tool));
                }
            }
        }
    }
}

struct CustomSkillTool {
    name: String,
    description: String,
    instructions: String,
}

impl CustomSkillTool {
    fn from_skill_md(name: &str, content: &str) -> Option<Self> {
        let description = extract_frontmatter_field(content, "description")
            .unwrap_or_else(|| format!("Custom skill: {}", name));

        let instructions = extract_body(content).to_string();

        Some(Self {
            name: format!("skill_{}", name),
            description,
            instructions,
        })
    }
}

fn extract_frontmatter_field(content: &str, field: &str) -> Option<String> {
    let content = content.trim();
    if !content.starts_with("---") {
        return None;
    }
    let after_first = &content[3..];
    let end = after_first.find("---")?;
    let frontmatter = &after_first[..end];

    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix(&format!("{}:", field)) {
            return Some(val.trim().to_string());
        }
    }
    None
}

fn extract_body(content: &str) -> &str {
    let content = content.trim();
    if !content.starts_with("---") {
        return content;
    }
    let after_first = &content[3..];
    if let Some(end) = after_first.find("---") {
        let rest = &after_first[end + 3..];
        rest.trim()
    } else {
        content
    }
}

#[async_trait]
impl Tool for CustomSkillTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "The task to accomplish using this skill"
                }
            },
            "required": ["task"]
        })
    }

    async fn execute(&self, args: &Value, _working_dir: &str) -> anyhow::Result<String> {
        let task = args["task"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'task' argument"))?;

        Ok(format!(
            "Skill '{}' instructions:\n\n{}\n\nTask: {}",
            self.name, self.instructions, task
        ))
    }
}
