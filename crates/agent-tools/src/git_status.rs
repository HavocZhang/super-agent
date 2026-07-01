use crate::Tool;
use async_trait::async_trait;
use serde_json::Value;

pub struct GitStatusTool;

#[async_trait]
impl Tool for GitStatusTool {
    fn name(&self) -> &str {
        "git_status"
    }

    fn description(&self) -> &str {
        "Show git working tree status"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    async fn execute(&self, _args: &Value, working_dir: &str) -> anyhow::Result<String> {
        let output = tokio::process::Command::new("git")
            .arg("status")
            .arg("--short")
            .current_dir(working_dir)
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() {
            return Ok(format!(
                "{}\n[stderr]\n{}",
                stdout, stderr
            ));
        }

        if stdout.is_empty() {
            Ok("Working tree clean.".to_string())
        } else {
            Ok(stdout.to_string())
        }
    }
}
