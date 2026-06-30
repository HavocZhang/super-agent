mod engine;

pub use engine::AgentEngine;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct AgentConfig {
    pub system_prompt: String,
    pub model: String,
    pub temperature: f64,
    pub max_tokens: u32,
    pub max_iterations: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            system_prompt: "You are a powerful coding agent. You can read and write files, execute shell commands, and search code. Always explain what you're doing before using tools.".to_string(),
            model: "gpt-4".to_string(),
            temperature: 0.7,
            max_tokens: 4096,
            max_iterations: 50,
        }
    }
}
