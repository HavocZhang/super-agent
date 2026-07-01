use agent_llm::{Message, Role};
use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: String,
    pub content: String,
    pub memory_type: String,
    pub importance: f64,
    pub created_at: String,
}

pub struct MemoryStore {
    db: sled::Db,
}

impl MemoryStore {
    pub fn new(path: &str) -> Result<Self> {
        let db = sled::open(path)?;
        Ok(Self { db })
    }

    pub fn in_memory() -> Result<Self> {
        let config = sled::Config::new().temporary(true);
        let db = config.open()?;
        Ok(Self { db })
    }

    pub fn store(&self, content: &str, memory_type: &str, importance: f64) -> Result<()> {
        let memory = Memory {
            id: Uuid::new_v4().to_string(),
            content: content.to_string(),
            memory_type: memory_type.to_string(),
            importance,
            created_at: Utc::now().to_rfc3339(),
        };

        let key = memory.id.clone();
        let value = serde_json::to_vec(&memory)?;
        self.db.insert(key.as_bytes(), value)?;
        Ok(())
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<Memory> {
        let query_lower = query.to_lowercase();
        let mut results: Vec<Memory> = Vec::new();

        for entry in self.db.iter() {
            if let Ok((_, value)) = entry {
                if let Ok(memory) = serde_json::from_slice::<Memory>(&value) {
                    if memory.content.to_lowercase().contains(&query_lower) {
                        results.push(memory);
                    }
                }
            }

            if results.len() >= limit {
                break;
            }
        }

        results.sort_by(|a, b| b.importance.partial_cmp(&a.importance).unwrap());
        results.truncate(limit);
        results
    }

    pub fn get_all(&self) -> Vec<Memory> {
        let mut results = Vec::new();
        for entry in self.db.iter() {
            if let Ok((_, value)) = entry {
                if let Ok(memory) = serde_json::from_slice::<Memory>(&value) {
                    results.push(memory);
                }
            }
        }
        results
    }

    pub fn count(&self) -> usize {
        self.db.len()
    }

    pub fn clear(&self) -> Result<()> {
        self.db.clear()?;
        Ok(())
    }

    pub fn extract_memories(&self, messages: &[Message]) -> Vec<String> {
        let mut memories = Vec::new();

        for msg in messages {
            if msg.role != Role::User && msg.role != Role::Assistant {
                continue;
            }

            let content = msg.content.to_lowercase();

            if is_preference(&content) {
                memories.push(msg.content.clone());
            } else if is_project_knowledge(&content) {
                memories.push(msg.content.clone());
            } else if is_error_experience(&content) {
                memories.push(msg.content.clone());
            }
        }

        memories
    }

    pub fn search_by_type(&self, memory_type: &str, limit: usize) -> Vec<Memory> {
        let mut results: Vec<Memory> = Vec::new();

        for entry in self.db.iter() {
            if let Ok((_, value)) = entry {
                if let Ok(memory) = serde_json::from_slice::<Memory>(&value) {
                    if memory.memory_type == memory_type {
                        results.push(memory);
                    }
                }
            }

            if results.len() >= limit {
                break;
            }
        }

        results.sort_by(|a, b| b.importance.partial_cmp(&a.importance).unwrap());
        results.truncate(limit);
        results
    }
}

const PREFERENCE_KEYWORDS: &[&str] = &[
    "i prefer",
    "i like",
    "i use",
    "i always",
    "i want",
    "我喜欢",
    "我偏好",
    "我习惯",
    "我用",
    "use pnpm",
    "use yarn",
    "use npm",
    "use bun",
    "4 spaces",
    "2 spaces",
    "tabs",
    "indent",
];

const PROJECT_KNOWLEDGE_KEYWORDS: &[&str] = &[
    "this project",
    "our codebase",
    "we use",
    "we have",
    "这个项目",
    "我们用",
    "我们的",
    "database is",
    "framework is",
    "language is",
    "stack is",
    "部署",
    "deploy",
    "architecture",
    "架构",
];

const ERROR_KEYWORDS: &[&str] = &[
    "will fail",
    "doesn't work",
    "doesn't support",
    "bug",
    "error because",
    "crash",
    "broken",
    "not compatible",
    "会失败",
    "不支持",
    "有 bug",
    "会报错",
];

fn contains_keyword(content: &str, keywords: &[&str]) -> bool {
    let content_lower = content.to_lowercase();
    for k in keywords {
        let k_lower = k.to_lowercase();
        let mut start = 0;
        while let Some(pos) = content_lower[start..].find(&k_lower) {
            let abs_pos = start + pos;
            let end = abs_pos + k_lower.len();
            let before_ok =
                abs_pos == 0 || !content_lower.as_bytes()[abs_pos - 1].is_ascii_alphanumeric();
            let after_ok = end >= content_lower.len()
                || !content_lower.as_bytes()[end].is_ascii_alphanumeric();
            if before_ok && after_ok {
                return true;
            }
            start = abs_pos + 1;
        }
    }
    false
}

fn is_preference(content: &str) -> bool {
    contains_keyword(content, PREFERENCE_KEYWORDS)
}

fn is_project_knowledge(content: &str) -> bool {
    contains_keyword(content, PROJECT_KNOWLEDGE_KEYWORDS)
}

fn is_error_experience(content: &str) -> bool {
    contains_keyword(content, ERROR_KEYWORDS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_memories_preference() {
        let store = MemoryStore::in_memory().unwrap();
        let messages = vec![
            Message::user("I prefer using pnpm over npm"),
            Message::assistant("Understood, noted."),
            Message::user("What's the weather?"),
        ];
        let memories = store.extract_memories(&messages);
        assert_eq!(memories.len(), 1);
        assert!(memories[0].contains("pnpm"));
    }

    #[test]
    fn test_extract_memories_project_knowledge() {
        let store = MemoryStore::in_memory().unwrap();
        let messages = vec![Message::user("This project uses Rust and PostgreSQL")];
        let memories = store.extract_memories(&messages);
        assert_eq!(memories.len(), 1);
    }

    #[test]
    fn test_extract_memories_error() {
        let store = MemoryStore::in_memory().unwrap();
        let messages = vec![Message::user(
            "That command will fail because permissions are denied",
        )];
        let memories = store.extract_memories(&messages);
        assert_eq!(memories.len(), 1);
    }

    #[test]
    fn test_search_by_type() {
        let store = MemoryStore::in_memory().unwrap();
        store.store("use 4 spaces", "preference", 0.9).unwrap();
        store.store("rust project", "project", 0.8).unwrap();
        store.store("2 space indent", "preference", 0.7).unwrap();

        let prefs = store.search_by_type("preference", 10);
        assert_eq!(prefs.len(), 2);
        assert!(prefs.iter().all(|m| m.memory_type == "preference"));
    }
}
