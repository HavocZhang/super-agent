use crate::Tool;
use crate::util::resolve_path;
use async_trait::async_trait;
use serde_json::Value;

pub struct FileReadTool;

#[async_trait]
impl Tool for FileReadTool {
    fn name(&self) -> &str {
        "file_read"
    }

    fn description(&self) -> &str {
        "Read the contents of a file"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to read"
                },
                "offset": {
                    "type": "integer",
                    "description": "Starting line number (0-based, default 0)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to read (default 2000)"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: &Value, working_dir: &str) -> anyhow::Result<String> {
        let path = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;
        let offset = args["offset"].as_u64().unwrap_or(0) as usize;
        let limit = args["limit"].as_u64().unwrap_or(2000) as usize;

        let resolved = resolve_path(path, working_dir);

        let bytes = tokio::fs::read(&resolved)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to read '{}': {}", path, e))?;

        let content = String::from_utf8_lossy(&bytes);

        let lines: Vec<&str> = content.lines().collect();
        let total = lines.len();

        let start = offset.min(total);
        let end = (start + limit).min(total);
        let selected = &lines[start..end];

        let mut result = String::new();
        for (i, line) in selected.iter().enumerate() {
            result.push_str(&format!("{}: {}\n", start + i + 1, line));
        }

        if end < total {
            result.push_str(&format!(
                "\n(Showing lines {}-{} of {} total. Use offset to read more.)",
                start + 1,
                end,
                total
            ));
        }

        Ok(result)
    }
}
