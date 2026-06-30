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
}
