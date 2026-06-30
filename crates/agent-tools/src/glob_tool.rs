use crate::Tool;
use async_trait::async_trait;
use serde_json::Value;

pub struct GlobTool;

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        "glob"
    }

    fn description(&self) -> &str {
        "Find files matching a glob pattern"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern (e.g., '**/*.rs', '*.toml')"
                },
                "path": {
                    "type": "string",
                    "description": "Directory to search in (defaults to current directory)"
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

        let _full_pattern = if path.ends_with('/') {
            format!("{}{}", path, pattern)
        } else {
            format!("{}/{}", path, pattern)
        };

        let output = tokio::process::Command::new("find")
            .arg(path)
            .arg("-name")
            .arg(pattern.replace("**/*", "*").replace("**/", ""))
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout);

        if stdout.is_empty() {
            Ok(format!("No files found matching '{}'", pattern))
        } else {
            Ok(stdout.to_string())
        }
    }
}
