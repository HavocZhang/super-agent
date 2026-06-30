use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;
use tracing::debug;

use crate::provider::LlmProvider;
use crate::types::{ChatRequest, ChatResponse, Role, ToolCall};

pub struct OpenAiProvider {
    client: Client,
    api_key: String,
    base_url: String,
}

impl OpenAiProvider {
    pub fn new(api_key: String, base_url: Option<String>) -> Self {
        Self {
            client: Client::new(),
            api_key,
            base_url: base_url.unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
        }
    }

    fn build_messages(&self, request: &ChatRequest) -> Vec<Value> {
        request
            .messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::Tool => "tool",
                };

                let mut msg = serde_json::json!({
                    "role": role,
                    "content": m.content
                });

                if let Some(tool_calls) = &m.tool_calls {
                    msg["tool_calls"] = serde_json::json!(
                        tool_calls
                            .iter()
                            .map(|tc| {
                                serde_json::json!({
                                    "id": tc.id,
                                    "type": "function",
                                    "function": {
                                        "name": tc.name,
                                        "arguments": serde_json::to_string(&tc.arguments).unwrap()
                                    }
                                })
                            })
                            .collect::<Vec<_>>()
                    );
                }

                if let Some(tool_call_id) = &m.tool_call_id {
                    msg["tool_call_id"] = serde_json::json!(tool_call_id);
                }

                msg
            })
            .collect()
    }

    fn build_tools(&self, request: &ChatRequest) -> Vec<Value> {
        request
            .tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters
                    }
                })
            })
            .collect()
    }
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse> {
        let messages = self.build_messages(&request);
        let tools = self.build_tools(&request);

        let body = serde_json::json!({
            "model": request.model,
            "messages": messages,
            "tools": tools,
            "temperature": request.temperature,
            "max_tokens": request.max_tokens
        });

        debug!("Sending request to {}/chat/completions", self.base_url);

        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to send request to LLM")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("LLM API error ({}): {}", status, error_text);
        }

        let data: Value = response
            .json()
            .await
            .context("Failed to parse LLM response")?;

        debug!("Response: {}", serde_json::to_string_pretty(&data).unwrap_or_default());

        let choice = data["choices"]
            .as_array()
            .and_then(|arr| arr.first())
            .context("No choices in response")?;

        let message = &choice["message"];

        if let Some(tool_calls) = message["tool_calls"].as_array() {
            let calls: Vec<ToolCall> = tool_calls
                .iter()
                .map(|tc| {
                    ToolCall {
                        id: tc["id"].as_str().unwrap_or_default().to_string(),
                        name: tc["function"]["name"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string(),
                        arguments: serde_json::from_str(
                            tc["function"]["arguments"].as_str().unwrap_or("{}"),
                        )
                        .unwrap_or_default(),
                    }
                })
                .collect();

            Ok(ChatResponse::ToolCall(calls))
        } else {
            let content = message["content"].as_str().unwrap_or("").to_string();
            Ok(ChatResponse::Text(content))
        }
    }
}
