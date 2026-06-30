use crate::Tool;
use async_trait::async_trait;
use serde_json::Value;

pub struct FileEditTool;

#[async_trait]
impl Tool for FileEditTool {
    fn name(&self) -> &str {
        "file_edit"
    }

    fn description(&self) -> &str {
        "Edit a file by replacing old_string with new_string"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to edit"
                },
                "old_string": {
                    "type": "string",
                    "description": "The exact string to find and replace"
                },
                "new_string": {
                    "type": "string",
                    "description": "The replacement string"
                }
            },
            "required": ["path", "old_string", "new_string"]
        })
    }

    async fn execute(&self, args: &Value) -> anyhow::Result<String> {
        let path = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;
        let old_string = args["old_string"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'old_string' argument"))?;
        let new_string = args["new_string"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'new_string' argument"))?;

        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to read '{}': {}", path, e))?;

        if !content.contains(old_string) {
            return Err(anyhow::anyhow!(
                "old_string not found in '{}'",
                path
            ));
        }

        let new_content = content.replacen(old_string, new_string, 1);
        tokio::fs::write(path, &new_content)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to write '{}': {}", path, e))?;

        Ok(format!("Successfully edited {}", path))
    }
}
