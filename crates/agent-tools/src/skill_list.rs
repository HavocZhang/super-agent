use crate::Tool;
use async_trait::async_trait;
use serde_json::Value;

pub struct SkillListTool;

#[async_trait]
impl Tool for SkillListTool {
    fn name(&self) -> &str {
        "skill_list"
    }

    fn description(&self) -> &str {
        "List all installed skills from ~/.codex/skills/ and ~/.agents/skills/"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    async fn execute(&self, _args: &Value, _working_dir: &str) -> anyhow::Result<String> {
        let mut results = Vec::new();

        let dirs = [
            dirs::home_dir().map(|h| h.join(".codex/skills")),
            dirs::home_dir().map(|h| h.join(".agents/skills")),
        ];

        for dir_opt in &dirs {
            if let Some(dir) = dir_opt {
                if dir.exists() {
                    let label = dir.display().to_string();
                    if let Ok(entries) = std::fs::read_dir(dir) {
                        let mut skills = Vec::new();
                        for entry in entries.flatten() {
                            let name = entry.file_name().to_string_lossy().to_string();
                            if name.starts_with('.') {
                                continue;
                            }
                            let has_skill_md = entry.path().join("SKILL.md").exists();
                            let marker = if has_skill_md { "✓" } else { "○" };
                            skills.push(format!("  {} {}", marker, name));
                        }
                        if !skills.is_empty() {
                            results.push(format!("{}:\n{}", label, skills.join("\n")));
                        }
                    }
                }
            }
        }

        if results.is_empty() {
            Ok("No skills found.".to_string())
        } else {
            Ok(results.join("\n\n"))
        }
    }
}
