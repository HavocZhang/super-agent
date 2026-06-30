use crate::Tool;
use async_trait::async_trait;
use serde_json::Value;

pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Search for a pattern in files using ripgrep or grep"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "The regex pattern to search for"
                },
                "path": {
                    "type": "string",
                    "description": "Directory or file to search in (defaults to current directory)"
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, args: &Value) -> anyhow::Result<String> {
        let pattern = args["pattern"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'pattern' argument"))?;
        let path = args["path"].as_str().unwrap_or(".");

        let output = tokio::process::Command::new("grep")
            .arg("-rn")
            .arg("--include=*")
            .arg(pattern)
            .arg(path)
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if stdout.is_empty() && stderr.is_empty() {
            Ok(format!("No matches found for '{}'", pattern))
        } else if stderr.is_empty() {
            Ok(stdout.to_string())
        } else {
            Ok(format!("{}\n[stderr]\n{}", stdout, stderr))
        }
    }
}
