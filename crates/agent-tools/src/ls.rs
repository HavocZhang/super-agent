use crate::Tool;
use async_trait::async_trait;
use serde_json::Value;

pub struct LsTool;

#[async_trait]
impl Tool for LsTool {
    fn name(&self) -> &str {
        "ls"
    }

    fn description(&self) -> &str {
        "List directory contents"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path to list (defaults to current directory)"
                },
                "show_hidden": {
                    "type": "boolean",
                    "description": "Whether to show hidden files (defaults to false)"
                }
            },
            "required": []
        })
    }

    async fn execute(&self, args: &Value) -> anyhow::Result<String> {
        let path = args["path"].as_str().unwrap_or(".");
        let show_hidden = args["show_hidden"].as_bool().unwrap_or(false);

        let mut cmd = tokio::process::Command::new("ls");
        cmd.arg("-la");

        if show_hidden {
            cmd.arg("-a");
        }

        cmd.arg(path);

        let output = cmd.output().await?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(stdout.to_string())
        } else {
            Ok(format!("{}\n[stderr]\n{}", stdout, stderr))
        }
    }
}
