mod file_edit;
mod file_read;
mod file_write;
mod glob_tool;
mod grep;
mod ls;
mod registry;
mod shell;

pub use registry::ToolRegistry;

use async_trait::async_trait;
use serde_json::Value;

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> Value;

    async fn execute(&self, args: &Value) -> anyhow::Result<String>;
}

pub fn default_tools() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(shell::ShellTool));
    registry.register(Box::new(file_read::FileReadTool));
    registry.register(Box::new(file_write::FileWriteTool));
    registry.register(Box::new(file_edit::FileEditTool));
    registry.register(Box::new(grep::GrepTool));
    registry.register(Box::new(glob_tool::GlobTool));
    registry.register(Box::new(ls::LsTool));
    registry
}
