use crate::Tool;
use async_trait::async_trait;
use serde_json::Value;

pub struct SkillReadTool;

#[async_trait]
impl Tool for SkillReadTool {
    fn name(&self) -> &str {
        "skill_read"
    }

    fn description(&self) -> &str {
        "Read the content of a specific skill's SKILL.md file by name"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "The skill name (e.g. 'golang-pro', 'lark-im')"
                }
            },
            "required": ["name"]
        })
    }

    async fn execute(&self, args: &Value, _working_dir: &str) -> anyhow::Result<String> {
        let name = args["name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'name' argument"))?;

        let dirs = [
            dirs::home_dir().map(|h| h.join(format!(".codex/skills/{}", name))),
            dirs::home_dir().map(|h| h.join(format!(".agents/skills/{}", name))),
        ];

        for dir_opt in &dirs {
            if let Some(dir) = dir_opt {
                let skill_file = dir.join("SKILL.md");
                if skill_file.exists() {
                    let content = tokio::fs::read_to_string(&skill_file).await?;
                    return Ok(format!(
                        "Skill '{}' from {}:\n\n{}",
                        name,
                        dir.display(),
                        content
                    ));
                }
            }
        }

        Ok(format!("Skill '{}' not found.", name))
    }
}
