use async_trait::async_trait;
use futures_util::Stream;
use serde_json::Value;
use std::pin::Pin;

use crate::types::{ChatRequest, ChatResponse};

#[derive(Debug, Clone)]
pub enum StreamEvent {
    Token(String),
    ToolCallStart { id: String, name: String },
    ToolCallDelta { id: String, arguments_delta: String },
    ToolCallEnd { id: String, name: String, arguments: Value },
    /// Snapshot of a file's content BEFORE a write/edit tool executes.
    /// The CLI can use this to compute diffs after the tool completes.
    ToolSnapshot { path: String, content: String },
    Done,
    Error(String),
}

pub type StreamResponse = Pin<Box<dyn Stream<Item = anyhow::Result<StreamEvent>> + Send>>;

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn chat(&self, request: ChatRequest) -> anyhow::Result<ChatResponse>;

    async fn chat_stream(&self, request: ChatRequest) -> anyhow::Result<StreamResponse> {
        let response = self.chat(request).await?;
        let events = match response {
            ChatResponse::Text(text) => vec![
                Ok(StreamEvent::Token(text)),
                Ok(StreamEvent::Done),
            ],
            ChatResponse::ToolCall(calls) => {
                let mut events = Vec::new();
                for call in &calls {
                    events.push(Ok(StreamEvent::ToolCallStart {
                        id: call.id.clone(),
                        name: call.name.clone(),
                    }));
                    events.push(Ok(StreamEvent::ToolCallEnd {
                        id: call.id.clone(),
                        name: call.name.clone(),
                        arguments: call.arguments.clone(),
                    }));
                }
                events.push(Ok(StreamEvent::Done));
                events
            }
        };
        Ok(Box::pin(futures_util::stream::iter(events)))
    }
}
