use crate::Tool;
use crate::util::resolve_path;
use async_trait::async_trait;
use serde_json::Value;
use std::path::Path;

pub struct FileWriteTool;

#[async_trait]
impl Tool for FileWriteTool {
    fn name(&self) -> &str {
        "file_write"
    }

    fn description(&self) -> &str {
        "Write content to a file (creates or overwrites)"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to write"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, args: &Value, working_dir: &str) -> anyhow::Result<String> {
        let path = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;
        let content = args["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'content' argument"))?;

        let resolved = resolve_path(path, working_dir);

        if let Some(parent) = Path::new(&resolved).parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent).await?;
            }
        }

        tokio::fs::write(&resolved, content.as_bytes())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to write '{}': {}", path, e))?;

        Ok(format!("Successfully written to {}", path))
    }
}
