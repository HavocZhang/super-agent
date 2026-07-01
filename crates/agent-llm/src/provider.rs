use async_trait::async_trait;
use futures_util::Stream;
use serde_json::Value;
use std::pin::Pin;

use crate::types::{ChatRequest, ChatResponse};

// ── LLM 提供者接口 ──────────────────────────────────────────
// 定义 LLM 调用的抽象接口，支持 OpenAI 兼容 API

/// 流式事件 —— 用于流式响应的各种事件类型
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// LLM 生成的文本 token
    Token(String),
    /// 工具调用开始
    ToolCallStart { id: String, name: String },
    /// 工具调用参数增量
    ToolCallDelta { id: String, arguments_delta: String },
    /// 工具调用结束（包含完整参数）
    ToolCallEnd { id: String, name: String, arguments: Value },
    /// 文件编辑前的快照，用于计算差异
    ToolSnapshot { path: String, content: String },
    /// 工具执行结果
    ToolResult { id: String, name: String, output: String },
    /// 流结束
    Done,
    /// 错误信息
    Error(String),
}

/// 流式响应类型：Pin<Box<dyn Stream<Item = Result<StreamEvent>>>>
pub type StreamResponse = Pin<Box<dyn Stream<Item = anyhow::Result<StreamEvent>> + Send>>;

/// LLM 提供者 trait —— 所有 LLM 后端的抽象接口
///
/// 当前实现：
/// - OpenAiProvider：兼容 OpenAI API 的提供者（也支持 DeepSeek、Ollama 等）
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// 非流式聊天调用
    async fn chat(&self, request: ChatRequest) -> anyhow::Result<ChatResponse>;

    /// 流式聊天调用（默认实现基于非流式调用封装）
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
