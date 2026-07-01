use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub agent_id: String,
    pub created_at: String,
    pub updated_at: String,
    pub message_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    pub role: String,
    pub content: String,
    pub tool_calls: Option<serde_json::Value>,
    pub tool_call_id: Option<String>,
    pub timestamp: String,
}

pub struct SessionStore {
    base_dir: PathBuf,
}

impl SessionStore {
    pub fn new() -> Result<Self> {
        let base_dir = dirs::home_dir()
            .context("Could not determine home directory")?
            .join(".agent")
            .join("sessions");
        std::fs::create_dir_all(&base_dir)?;
        Ok(Self { base_dir })
    }

    pub fn new_at(base: &std::path::Path) -> Result<Self> {
        let base_dir = base.join("sessions");
        std::fs::create_dir_all(&base_dir)?;
        Ok(Self { base_dir })
    }

    pub fn create(&self, agent_id: &str) -> Result<Session> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let session = Session {
            id: id.clone(),
            title: "New Session".to_string(),
            agent_id: agent_id.to_string(),
            created_at: now.clone(),
            updated_at: now,
            message_count: 0,
        };
        let session_dir = self.base_dir.join(&id);
        std::fs::create_dir_all(&session_dir)?;
        let session_json = serde_json::to_string_pretty(&session)?;
        std::fs::write(session_dir.join("session.json"), session_json)?;
        std::fs::write(session_dir.join("messages.jsonl"), "")?;
        Ok(session)
    }

    pub fn get(&self, id: &str) -> Result<Session> {
        let path = self.base_dir.join(id).join("session.json");
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Session '{}' not found", id))?;
        let session: Session = serde_json::from_str(&content)?;
        Ok(session)
    }

    pub fn list(&self) -> Result<Vec<Session>> {
        let mut sessions = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.base_dir) {
            for entry in entries.flatten() {
                let session_file = entry.path().join("session.json");
                if session_file.exists() {
                    if let Ok(content) = std::fs::read_to_string(&session_file) {
                        if let Ok(session) = serde_json::from_str::<Session>(&content) {
                            sessions.push(session);
                        }
                    }
                }
            }
        }
        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(sessions)
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let session_dir = self.base_dir.join(id);
        if session_dir.exists() {
            std::fs::remove_dir_all(session_dir)?;
        }
        Ok(())
    }

    pub fn append_message(&self, session_id: &str, msg: &SessionMessage) -> Result<()> {
        let messages_path = self.base_dir.join(session_id).join("messages.jsonl");
        let mut line = serde_json::to_string(msg)?;
        line.push('\n');
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&messages_path)
            .and_then(|mut f| {
                use std::io::Write;
                f.write_all(line.as_bytes())
            })?;
        // Update message count
        if let Ok(mut session) = self.get(session_id) {
            session.message_count += 1;
            session.updated_at = Utc::now().to_rfc3339();
            let session_json = serde_json::to_string_pretty(&session)?;
            std::fs::write(
                self.base_dir.join(session_id).join("session.json"),
                session_json,
            )?;
        }
        Ok(())
    }

    pub fn get_messages(&self, session_id: &str) -> Result<Vec<SessionMessage>> {
        let messages_path = self.base_dir.join(session_id).join("messages.jsonl");
        let content = std::fs::read_to_string(&messages_path)?;
        let mut messages = Vec::new();
        for line in content.lines() {
            if !line.trim().is_empty() {
                if let Ok(msg) = serde_json::from_str::<SessionMessage>(line) {
                    messages.push(msg);
                }
            }
        }
        Ok(messages)
    }

    pub fn update_title(&self, id: &str, title: &str) -> Result<()> {
        let mut session = self.get(id)?;
        session.title = title.to_string();
        session.updated_at = Utc::now().to_rfc3339();
        let session_json = serde_json::to_string_pretty(&session)?;
        std::fs::write(
            self.base_dir.join(id).join("session.json"),
            session_json,
        )?;
        Ok(())
    }
}
