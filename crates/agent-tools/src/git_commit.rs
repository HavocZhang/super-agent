use crate::Tool;
use async_trait::async_trait;
use serde_json::Value;

pub struct GitCommitTool;

#[async_trait]
impl Tool for GitCommitTool {
    fn name(&self) -> &str {
        "git_commit"
    }

    fn description(&self) -> &str {
        "Stage files and create a git commit"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "message": {
                    "type": "string",
                    "description": "The commit message"
                },
                "files": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Files to stage (optional, defaults to all changes)"
                }
            },
            "required": ["message"]
        })
    }

    async fn execute(&self, args: &Value, working_dir: &str) -> anyhow::Result<String> {
        let message = args["message"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'message' argument"))?;

        // Stage files
        let mut add_cmd = tokio::process::Command::new("git");
        add_cmd.arg("add").current_dir(working_dir);

        if let Some(files) = args["files"].as_array() {
            if files.is_empty() {
                add_cmd.arg("-A");
            } else {
                for f in files {
                    if let Some(s) = f.as_str() {
                        add_cmd.arg(s);
                    }
                }
            }
        } else {
            add_cmd.arg("-A");
        }

        let add_output = add_cmd.output().await?;
        if !add_output.status.success() {
            let stderr = String::from_utf8_lossy(&add_output.stderr);
            return Ok(format!("git add failed:\n{}", stderr));
        }

        // Commit
        let commit_output = tokio::process::Command::new("git")
            .arg("commit")
            .arg("-m")
            .arg(message)
            .current_dir(working_dir)
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&commit_output.stdout);
        let stderr = String::from_utf8_lossy(&commit_output.stderr);

        if commit_output.status.success() {
            Ok(stdout.to_string())
        } else {
            Ok(format!("{}\n[stderr]\n{}", stdout, stderr))
        }
    }
}
