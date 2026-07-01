use crate::Tool;
use async_trait::async_trait;
use serde_json::Value;
use std::time::Duration;

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
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Timeout in milliseconds (default 30000)"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, args: &Value, working_dir: &str) -> anyhow::Result<String> {
        let command = args["command"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'command' argument"))?;
        let timeout_ms = args["timeout_ms"].as_u64().unwrap_or(30000);

        let result = tokio::time::timeout(
            Duration::from_millis(timeout_ms),
            tokio::process::Command::new("sh")
                .arg("-c")
                .arg(command)
                .current_dir(working_dir)
                .output(),
        )
        .await;

        let output = match result {
            Ok(inner) => inner?,
            Err(_) => {
                return Ok(format!(
                    "[timeout] Command timed out after {}ms",
                    timeout_ms
                ));
            }
        };

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
