use agent_llm::{ChatRequest, ChatResponse, LlmProvider, Message, Role};
use agent_memory::MemoryStore;
use anyhow::Result;
use std::sync::Arc;
use tracing::debug;

pub struct LearningLoop {
    memory: Arc<MemoryStore>,
    llm: Arc<Box<dyn LlmProvider>>,
    nudge_interval: usize,
    turn_counter: usize,
    model: String,
}

pub struct MemoryEntry {
    pub content: String,
    pub memory_type: String,
    pub importance: f64,
}

pub struct SessionSummary {
    pub session_id: String,
    pub summary: String,
    pub key_decisions: Vec<String>,
}

impl LearningLoop {
    pub fn new(memory: Arc<MemoryStore>, llm: Arc<Box<dyn LlmProvider>>, model: Option<String>) -> Self {
        Self {
            memory,
            llm,
            nudge_interval: 5,
            turn_counter: 0,
            model: model.unwrap_or_else(|| "gpt-4".to_string()),
        }
    }

    pub fn with_nudge_interval(mut self, interval: usize) -> Self {
        self.nudge_interval = interval;
        self
    }

    pub async fn maybe_nudge(&mut self, messages: &[Message]) -> Option<String> {
        self.turn_counter += 1;

        if self.turn_counter % self.nudge_interval != 0 {
            return None;
        }

        let conversation = format_messages(messages);

        let prompt = format!(
            "Analyze the following conversation and determine if there are any \
             user preferences, project knowledge, error experiences, or successful \
             patterns worth remembering. Reply with EXACTLY one of:\n\n\
             NOTABLE: <brief summary of what's worth remembering>\n\
             NOTHING: no notable information to remember\n\n\
             Conversation:\n{}",
            conversation
        );

        let request = ChatRequest {
            model: self.model.clone(),
            messages: vec![Message::system(&prompt)],
            tools: vec![],
            temperature: 0.3,
            max_tokens: 200,
        };

        let response = match self.llm.chat(request).await {
            Ok(ChatResponse::Text(text)) => text,
            _ => return None,
        };

        let trimmed = response.trim();
        if trimmed.starts_with("NOTABLE:") {
            let summary = trimmed.strip_prefix("NOTABLE:").unwrap().trim().to_string();
            debug!("Nudge triggered: {}", summary);
            Some(format!(
                "I noticed some information worth remembering: {}\nWould you like me to save this?",
                summary
            ))
        } else {
            None
        }
    }

    pub async fn extract_memories(&self, messages: &[Message]) -> Vec<MemoryEntry> {
        let conversation = format_messages(messages);

        let prompt = format!(
            "Analyze the following conversation and extract memories. For each memory, provide:\n\
             - type: one of (preference, project, error, success)\n\
             - importance: a float from 0.0 to 1.0\n\
             - content: the memory content\n\n\
             Return a JSON array of objects with keys: type, importance, content.\n\
             If nothing worth remembering, return an empty array [].\n\n\
             Conversation:\n{}",
            conversation
        );

        let request = ChatRequest {
            model: self.model.clone(),
            messages: vec![Message::system(&prompt)],
            tools: vec![],
            temperature: 0.2,
            max_tokens: 1000,
        };

        let response = match self.llm.chat(request).await {
            Ok(ChatResponse::Text(text)) => text,
            _ => return vec![],
        };

        let json_str = extract_json_array(&response);
        let entries: Vec<serde_json::Value> = match serde_json::from_str(json_str) {
            Ok(v) => v,
            Err(_) => return vec![],
        };

        entries
            .into_iter()
            .filter_map(|v| {
                let content = v.get("content")?.as_str()?.to_string();
                let memory_type = v.get("type")?.as_str()?.to_string();
                let importance = v.get("importance")?.as_f64()?;
                Some(MemoryEntry {
                    content,
                    memory_type,
                    importance,
                })
            })
            .collect()
    }

    pub async fn summarize_session(&self, messages: &[Message]) -> String {
        let conversation = format_messages(messages);

        let prompt = format!(
            "Summarize the following conversation in 2-3 sentences. Focus on:\n\
             - What was discussed or accomplished\n\
             - Key decisions made\n\
             - Any important context for future reference\n\n\
             Conversation:\n{}",
            conversation
        );

        let request = ChatRequest {
            model: self.model.clone(),
            messages: vec![Message::system(&prompt)],
            tools: vec![],
            temperature: 0.3,
            max_tokens: 300,
        };

        match self.llm.chat(request).await {
            Ok(ChatResponse::Text(text)) => text,
            _ => "Unable to summarize session.".to_string(),
        }
    }

    pub fn search_sessions(&self, query: &str) -> Vec<SessionSummary> {
        let memories = self.memory.search(query, 10);
        memories
            .into_iter()
            .map(|m| SessionSummary {
                session_id: m.id,
                summary: m.content,
                key_decisions: vec![],
            })
            .collect()
    }

    pub fn persist_entries(&self, entries: &[MemoryEntry]) -> Result<()> {
        for entry in entries {
            self.memory
                .store(&entry.content, &entry.memory_type, entry.importance)?;
        }
        Ok(())
    }
}

fn format_messages(messages: &[Message]) -> String {
    messages
        .iter()
        .filter(|m| m.role == Role::User || m.role == Role::Assistant)
        .map(|m| {
            let role = match m.role {
                Role::User => "User",
                Role::Assistant => "Assistant",
                _ => unreachable!(),
            };
            format!("{}: {}", role, m.content)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn extract_json_array(text: &str) -> &str {
    if let Some(start) = text.find('[') {
        if let Some(end) = text.rfind(']') {
            return &text[start..=end];
        }
    }
    "[]"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_array() {
        let text = "Here is the result: [{\"key\": \"value\"}] done.";
        assert_eq!(extract_json_array(text), "[{\"key\": \"value\"}]");
    }

    #[test]
    fn test_extract_json_array_none() {
        assert_eq!(extract_json_array("no json here"), "[]");
    }

    #[test]
    fn test_format_messages() {
        let messages = vec![
            Message::system("system prompt"),
            Message::user("hello"),
            Message::assistant("hi there"),
        ];
        let formatted = format_messages(&messages);
        assert!(formatted.contains("User: hello"));
        assert!(formatted.contains("Assistant: hi there"));
        assert!(!formatted.contains("system prompt"));
    }
}
