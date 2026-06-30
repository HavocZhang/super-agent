use crate::Tool;
use async_trait::async_trait;
use serde_json::Value;

pub struct ShellTool;

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        "Execute a shell command and return stdout/stderr"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, args: &Value) -> anyhow::Result<String> {
        let command = args["command"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'command' argument"))?;

        let output = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            if stdout.is_empty() && stderr.is_empty() {
                Ok("(command completed successfully with no output)".to_string())
            } else if stderr.is_empty() {
                Ok(stdout.to_string())
            } else {
                Ok(format!("{}\n[stderr]\n{}", stdout, stderr))
            }
        } else {
            let code = output.status.code().unwrap_or(-1);
            Ok(format!(
                "{}\n[stderr]\n{}\n[exit code: {}]",
                stdout, stderr, code
            ))
        }
    }
}
