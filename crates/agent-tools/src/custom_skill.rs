use crate::Tool;
use async_trait::async_trait;
use serde_json::Value;

/// A tool dynamically created from a SKILL.md file.
/// Skills provide instructions for how to accomplish specific tasks.
pub struct CustomSkillTool {
    name: String,
    description: String,
    instructions: String,
}

impl CustomSkillTool {
    /// Parse a SKILL.md file and create a tool from it.
    /// The frontmatter (delimited by `---`) can contain metadata like `description:`.
    /// Everything after the frontmatter is treated as instructions.
    pub fn from_skill_md(name: &str, content: &str) -> Option<Self> {
        let description = extract_frontmatter_field(content, "description")
            .unwrap_or_else(|| format!("Custom skill: {}", name));

        let instructions = extract_body(content).to_string();

        Some(Self {
            name: format!("skill_{}", name),
            description,
            instructions,
        })
    }
}

#[async_trait]
impl Tool for CustomSkillTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "The task to accomplish using this skill"
                }
            },
            "required": ["task"]
        })
    }

    async fn execute(&self, args: &Value, _working_dir: &str) -> anyhow::Result<String> {
        let task = args["task"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'task' argument"))?;

        Ok(format!(
            "Skill '{}' instructions:\n\n{}\n\nTask: {}",
            self.name, self.instructions, task
        ))
    }
}

/// Extract a field from YAML-like frontmatter.
/// Frontmatter is delimited by `---` at the start of the file.
///
/// # Example
///
/// ```text
/// ---
/// description: A helpful skill
/// ---
/// Body text here
/// ```
pub fn extract_frontmatter_field(content: &str, field: &str) -> Option<String> {
    let content = content.trim();
    if !content.starts_with("---") {
        return None;
    }
    let after_first = &content[3..];
    let end = after_first.find("---")?;
    let frontmatter = &after_first[..end];

    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix(&format!("{}:", field)) {
            return Some(val.trim().to_string());
        }
    }
    None
}

/// Extract the body content (after frontmatter) from a SKILL.md file.
pub fn extract_body(content: &str) -> &str {
    let content = content.trim();
    if !content.starts_with("---") {
        return content;
    }
    let after_first = &content[3..];
    if let Some(end) = after_first.find("---") {
        let rest = &after_first[end + 3..];
        rest.trim()
    } else {
        content
    }
}
