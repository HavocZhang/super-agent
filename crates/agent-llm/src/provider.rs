use async_trait::async_trait;

use crate::types::{ChatRequest, ChatResponse};

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn chat(&self, request: ChatRequest) -> anyhow::Result<ChatResponse>;
}
