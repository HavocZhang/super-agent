use crate::Tool;
use async_trait::async_trait;
use serde_json::Value;

pub struct GitDiffTool;

#[async_trait]
impl Tool for GitDiffTool {
    fn name(&self) -> &str {
        "git_diff"
    }

    fn description(&self) -> &str {
        "Show git diff of unstaged and staged changes"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File or directory to diff (optional, defaults to all)"
                }
            },
            "required": []
        })
    }

    async fn execute(&self, args: &Value, working_dir: &str) -> anyhow::Result<String> {
        let path = args["path"].as_str();

        let mut output = String::new();

        // Unstaged changes
        let mut cmd = tokio::process::Command::new("git");
        cmd.arg("diff").current_dir(working_dir);
        if let Some(p) = path {
            cmd.arg(p);
        }
        let unstaged = cmd.output().await?;
        let unstaged_str = String::from_utf8_lossy(&unstaged.stdout);
        if !unstaged_str.is_empty() {
            output.push_str("--- Unstaged changes ---\n");
            output.push_str(&unstaged_str);
        }

        // Staged changes
        let mut cmd = tokio::process::Command::new("git");
        cmd.arg("diff").arg("--cached").current_dir(working_dir);
        if let Some(p) = path {
            cmd.arg(p);
        }
        let staged = cmd.output().await?;
        let staged_str = String::from_utf8_lossy(&staged.stdout);
        if !staged_str.is_empty() {
            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str("--- Staged changes ---\n");
            output.push_str(&staged_str);
        }

        if output.is_empty() {
            Ok("No changes.".to_string())
        } else {
            Ok(output)
        }
    }
}
