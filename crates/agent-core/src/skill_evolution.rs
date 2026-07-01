use agent_llm::{ChatRequest, ChatResponse, LlmProvider, Message};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub content: String,
    pub version: u32,
    pub created_at: String,
    pub usage_count: u32,
    pub success_count: u32,
}

pub struct SkillEvolution {
    skills_dir: PathBuf,
    llm: Arc<Box<dyn LlmProvider>>,
}

impl SkillEvolution {
    pub fn new(skills_dir: &str, llm: Arc<Box<dyn LlmProvider>>) -> Self {
        Self {
            skills_dir: PathBuf::from(skills_dir),
            llm,
        }
    }

    pub fn default_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".agent")
            .join("skills")
    }

    pub async fn maybe_create_skill(
        &self,
        conversation: &[Message],
        result: &str,
    ) -> Option<Skill> {
        let tool_call_count: usize = conversation
            .iter()
            .filter_map(|m| m.tool_calls.as_ref())
            .map(|calls| calls.len())
            .sum();

        if tool_call_count < 5 {
            debug!(
                "Skipping skill creation: only {} tool calls (need >= 5)",
                tool_call_count
            );
            return None;
        }

        if result.contains("Error") || result.contains("failed") {
            debug!("Skipping skill creation: task did not succeed");
            return None;
        }

        info!(
            "Analyzing conversation with {} tool calls for skill extraction",
            tool_call_count
        );

        let conversation_text = self.format_conversation(conversation);

        let prompt = format!(
            r#"Analyze the following conversation and extract a reusable skill pattern.

Conversation:
{conversation_text}

Task result: {result}

Create a SKILL.md in this exact format:
---
name: <kebab-case-name>
description: <one-line description>
version: 1
usage_count: 0
success_count: 0
---

# <Skill Name>

## When to use
- <scenario 1>
- <scenario 2>

## Steps
1. <step 1>
2. <step 2>

## Example
<brief example showing how to apply this skill>

Rules:
- Name must be kebab-case, max 40 chars
- Description must be one sentence
- Steps should capture the reusable pattern, not the specific instance
- Keep it concise and actionable
- Return ONLY the SKILL.md content, nothing else"#
        );

        let request = ChatRequest {
            model: "gpt-4".to_string(),
            messages: vec![Message::user(&prompt)],
            tools: vec![],
            temperature: 0.3,
            max_tokens: 2048,
        };

        let response = match self.llm.chat(request).await {
            Ok(ChatResponse::Text(text)) => text,
            Ok(ChatResponse::ToolCall(_)) => {
                debug!("Unexpected tool call from skill extraction LLM");
                return None;
            }
            Err(e) => {
                debug!("LLM call failed during skill extraction: {}", e);
                return None;
            }
        };

        let content = response.trim();
        let (name, description) = self.parse_frontmatter(content)?;

        let skill = Skill {
            name,
            description,
            content: content.to_string(),
            version: 1,
            created_at: chrono::Utc::now().to_rfc3339(),
            usage_count: 0,
            success_count: 0,
        };

        if let Err(e) = self.save_skill(&skill) {
            debug!("Failed to save skill: {}", e);
            return None;
        }

        info!("Created new skill: {}", skill.name);
        Some(skill)
    }

    pub fn record_usage(&self, skill_name: &str, success: bool) {
        let skill_path = self.skill_path(skill_name);
        let md_path = skill_path.join("SKILL.md");

        if !md_path.exists() {
            debug!("Skill '{}' not found for usage recording", skill_name);
            return;
        }

        if let Ok(content) = std::fs::read_to_string(&md_path) {
            let mut updated = content.clone();

            if let Some(count) = self.extract_field_u32(&content, "usage_count") {
                let new_count = count + 1;
                updated = updated.replace(
                    &format!("usage_count: {}", count),
                    &format!("usage_count: {}", new_count),
                );
            }

            if success {
                if let Some(count) = self.extract_field_u32(&updated, "success_count") {
                    let new_count = count + 1;
                    updated = updated.replace(
                        &format!("success_count: {}", count),
                        &format!("success_count: {}", new_count),
                    );
                }
            }

            if updated != content {
                let _ = std::fs::write(&md_path, updated);
            }
        }
    }

    pub fn load_skill(&self, name: &str) -> Option<Skill> {
        let skill_path = self.skill_path(name);
        let md_path = skill_path.join("SKILL.md");

        if !md_path.exists() {
            return None;
        }

        let content = std::fs::read_to_string(&md_path).ok()?;
        let (parsed_name, description) = self.parse_frontmatter(&content).unwrap_or_else(|| {
            (
                name.to_string(),
                "No description".to_string(),
            )
        });

        let version = self.extract_field_u32(&content, "version").unwrap_or(1);
        let usage_count = self
            .extract_field_u32(&content, "usage_count")
            .unwrap_or(0);
        let success_count = self
            .extract_field_u32(&content, "success_count")
            .unwrap_or(0);
        let created_at = self
            .extract_field_str(&content, "created_at")
            .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

        Some(Skill {
            name: parsed_name,
            description,
            content,
            version,
            created_at,
            usage_count,
            success_count,
        })
    }

    pub fn save_skill(&self, skill: &Skill) -> Result<()> {
        let skill_path = self.skill_path(&skill.name);
        std::fs::create_dir_all(&skill_path)
            .with_context(|| format!("Failed to create skill dir: {}", skill_path.display()))?;

        let md_path = skill_path.join("SKILL.md");
        std::fs::write(&md_path, &skill.content)
            .with_context(|| format!("Failed to write SKILL.md: {}", md_path.display()))?;

        Ok(())
    }

    pub fn list_custom_skills(&self) -> Vec<Skill> {
        let mut skills = Vec::new();

        if !self.skills_dir.exists() {
            return skills;
        }

        if let Ok(entries) = std::fs::read_dir(&self.skills_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') {
                    continue;
                }
                if let Some(skill) = self.load_skill(&name) {
                    skills.push(skill);
                }
            }
        }

        skills
    }

    fn skill_path(&self, name: &str) -> PathBuf {
        self.skills_dir.join(name)
    }

    fn format_conversation(&self, messages: &[Message]) -> String {
        messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    agent_llm::Role::System => "system",
                    agent_llm::Role::User => "user",
                    agent_llm::Role::Assistant => "assistant",
                    agent_llm::Role::Tool => "tool",
                };
                let mut parts = vec![format!("[{}]: {}", role, m.content)];
                if let Some(calls) = &m.tool_calls {
                    for call in calls {
                        parts.push(format!("  -> tool_call: {}({})", call.name, call.arguments));
                    }
                }
                parts.join("\n")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn parse_frontmatter(&self, content: &str) -> Option<(String, String)> {
        let content = content.trim();
        if !content.starts_with("---") {
            return None;
        }

        let after_first = &content[3..];
        let end = after_first.find("---")?;
        let frontmatter = &after_first[..end];

        let mut name = None;
        let mut description = None;

        for line in frontmatter.lines() {
            let line = line.trim();
            if let Some(val) = line.strip_prefix("name:") {
                name = Some(val.trim().to_string());
            } else if let Some(val) = line.strip_prefix("description:") {
                description = Some(val.trim().to_string());
            }
        }

        Some((name?, description?))
    }

    fn extract_field_u32(&self, content: &str, field: &str) -> Option<u32> {
        for line in content.lines() {
            let line = line.trim();
            if let Some(val) = line.strip_prefix(&format!("{}:", field)) {
                return val.trim().parse().ok();
            }
        }
        None
    }

    fn extract_field_str(&self, content: &str, field: &str) -> Option<String> {
        for line in content.lines() {
            let line = line.trim();
            if let Some(val) = line.strip_prefix(&format!("{}:", field)) {
                return Some(val.trim().to_string());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_frontmatter() {
        let se = SkillEvolution {
            skills_dir: PathBuf::from("/tmp"),
            llm: Arc::new(Box::new(MockLlm)),
        };

        let content = r#"---
name: test-skill
description: A test skill
version: 1
usage_count: 0
success_count: 0
---

# Test Skill

## When to use
- Testing

## Steps
1. Do something"#;

        let (name, desc) = se.parse_frontmatter(content).unwrap();
        assert_eq!(name, "test-skill");
        assert_eq!(desc, "A test skill");
    }

    #[test]
    fn test_parse_frontmatter_invalid() {
        let se = SkillEvolution {
            skills_dir: PathBuf::from("/tmp"),
            llm: Arc::new(Box::new(MockLlm)),
        };

        assert!(se.parse_frontmatter("no frontmatter here").is_none());
        assert!(se.parse_frontmatter("---\nname: foo").is_none());
    }

    #[test]
    fn test_extract_field_u32() {
        let se = SkillEvolution {
            skills_dir: PathBuf::from("/tmp"),
            llm: Arc::new(Box::new(MockLlm)),
        };

        let content = "version: 3\nusage_count: 42\nsuccess_count: 7";
        assert_eq!(se.extract_field_u32(content, "version"), Some(3));
        assert_eq!(se.extract_field_u32(content, "usage_count"), Some(42));
        assert_eq!(se.extract_field_u32(content, "success_count"), Some(7));
        assert_eq!(se.extract_field_u32(content, "missing"), None);
    }

    struct MockLlm;

    #[async_trait::async_trait]
    impl agent_llm::LlmProvider for MockLlm {
        async fn chat(
            &self,
            _request: agent_llm::ChatRequest,
        ) -> anyhow::Result<agent_llm::ChatResponse> {
            Ok(agent_llm::ChatResponse::Text("mock".to_string()))
        }
    }
}
