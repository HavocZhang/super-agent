use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;
use tracing::debug;

use crate::provider::{LlmProvider, StreamEvent, StreamResponse};
use crate::types::{ChatRequest, ChatResponse, Role, ToolCall};

pub struct OpenAiProvider {
    client: Client,
    api_key: String,
    base_url: String,
}

impl OpenAiProvider {
    pub fn new(api_key: String, base_url: Option<String>) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| Client::new());
        Self {
            client,
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

    fn build_body(&self, request: &ChatRequest, stream: bool) -> Value {
        let messages = self.build_messages(request);
        let tools = self.build_tools(request);
        let mut body = serde_json::json!({
            "model": request.model,
            "messages": messages,
            "tools": tools,
            "temperature": request.temperature,
            "max_tokens": request.max_tokens
        });
        if stream {
            body["stream"] = serde_json::json!(true);
            body["stream_options"] = serde_json::json!({ "include_usage": true });
        }
        body
    }
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse> {
        let body = self.build_body(&request, false);

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

        let bytes = response
            .bytes()
            .await
            .context("Failed to read LLM response body")?;

        let text = String::from_utf8_lossy(&bytes);

        let data: Value =
            serde_json::from_str(&text).context("Failed to parse LLM response as JSON")?;

        debug!(
            "Response: {}",
            serde_json::to_string_pretty(&data).unwrap_or_default()
        );

        let choice = data["choices"]
            .as_array()
            .and_then(|arr| arr.first())
            .context("No choices in response")?;

        let message = &choice["message"];

        if let Some(tool_calls) = message["tool_calls"].as_array() {
            let calls: Vec<ToolCall> = tool_calls
                .iter()
                .map(|tc| ToolCall {
                    id: tc["id"].as_str().unwrap_or_default().to_string(),
                    name: tc["function"]["name"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    arguments: serde_json::from_str(
                        tc["function"]["arguments"].as_str().unwrap_or("{}"),
                    )
                    .unwrap_or_default(),
                })
                .collect();

            Ok(ChatResponse::ToolCall(calls))
        } else {
            let content = message["content"].as_str().unwrap_or("").to_string();
            Ok(ChatResponse::Text(content))
        }
    }

    async fn chat_stream(&self, request: ChatRequest) -> Result<StreamResponse> {
        let body = self.build_body(&request, true);

        debug!("Sending stream request to {}/chat/completions", self.base_url);

        // Retry up to 3 times on transient failures
        let mut last_err = None;
        let response = {
            let mut resp = None;
            for attempt in 0..3 {
                match self.client
                    .post(format!("{}/chat/completions", self.base_url))
                    .header("Authorization", format!("Bearer {}", self.api_key))
                    .header("Content-Type", "application/json")
                    .json(&body)
                    .send()
                    .await
                {
                    Ok(r) if r.status().is_success() => {
                        resp = Some(r);
                        break;
                    }
                    Ok(r) => {
                        let status = r.status();
                        let text = r.text().await.unwrap_or_default();
                        last_err = Some(format!("LLM API error ({}): {}", status, text));
                        if status.as_u16() >= 500 || status.as_u16() == 429 {
                            tokio::time::sleep(std::time::Duration::from_secs(1 << attempt)).await;
                            continue;
                        }
                        anyhow::bail!("LLM API error ({}): {}", status, text);
                    }
                    Err(e) => {
                        last_err = Some(format!("Request failed: {}", e));
                        tokio::time::sleep(std::time::Duration::from_secs(1 << attempt)).await;
                    }
                }
            }
            resp.ok_or_else(|| anyhow::anyhow!("{}", last_err.unwrap_or_else(|| "Unknown error".to_string())))?
        };

        let byte_stream = response.bytes_stream();

        let stream = async_stream::try_stream! {
            use futures_util::StreamExt;

            let mut buffer = String::new();
            let mut tool_calls: Vec<(String, String, String)> = Vec::new();

            tokio::pin!(byte_stream);
            while let Some(chunk_result) = byte_stream.next().await {
                let chunk = chunk_result.map_err(|e| anyhow::anyhow!("Stream read error: {}", e))?;
                buffer.push_str(&String::from_utf8_lossy(&chunk));

                while let Some(line_end) = buffer.find('\n') {
                    let line = buffer[..line_end].trim().to_string();
                    buffer = buffer[line_end + 1..].to_string();

                    if line.is_empty() || line.starts_with(':') {
                        continue;
                    }

                    let Some(data_str) = line.strip_prefix("data: ") else {
                        continue;
                    };

                    if data_str == "[DONE]" {
                        if !tool_calls.is_empty() {
                            for (id, name, args_str) in &tool_calls {
                                let arguments: Value = serde_json::from_str(args_str)
                                    .unwrap_or_else(|_| serde_json::json!({}));
                                yield StreamEvent::ToolCallEnd {
                                    id: id.clone(),
                                    name: name.clone(),
                                    arguments,
                                };
                            }
                            tool_calls.clear();
                        }
                        yield StreamEvent::Done;
                        return;
                    }

                    let data: Value = match serde_json::from_str(data_str) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };

                    if let Some(choices) = data["choices"].as_array() {
                        if let Some(choice) = choices.first() {
                            let delta = &choice["delta"];

                            if let Some(content) = delta["content"].as_str() {
                                if !content.is_empty() {
                                    yield StreamEvent::Token(content.to_string());
                                }
                            }

                            if let Some(tc_array) = delta["tool_calls"].as_array() {
                                for tc in tc_array {
                                    let index = tc["index"].as_u64().unwrap_or(0) as usize;
                                    let id = tc["id"].as_str().unwrap_or("").to_string();
                                    let name = tc["function"]["name"]
                                        .as_str()
                                        .unwrap_or("")
                                        .to_string();
                                    let args_delta = tc["function"]["arguments"]
                                        .as_str()
                                        .unwrap_or("")
                                        .to_string();

                                    if !id.is_empty() && !name.is_empty() {
                                        while tool_calls.len() <= index {
                                            tool_calls.push((String::new(), String::new(), String::new()));
                                        }
                                        tool_calls[index].0 = id.clone();
                                        tool_calls[index].1 = name.clone();
                                        yield StreamEvent::ToolCallStart { id, name };
                                    }

                                    if !args_delta.is_empty() {
                                        if tool_calls.len() > index {
                                            tool_calls[index].2.push_str(&args_delta);
                                        }
                                        yield StreamEvent::ToolCallDelta {
                                            id: if tool_calls.len() > index {
                                                tool_calls[index].0.clone()
                                            } else {
                                                String::new()
                                            },
                                            arguments_delta: args_delta,
                                        };
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Handle remaining buffer (some APIs don't send [DONE])
            // Handle remaining buffer (some APIs don't send [DONE])
            if !tool_calls.is_empty() {
                for (id, name, args_str) in &tool_calls {
                    let arguments: Value = serde_json::from_str(args_str)
                        .unwrap_or_else(|_| serde_json::json!({}));
                    yield StreamEvent::ToolCallEnd {
                        id: id.clone(),
                        name: name.clone(),
                        arguments,
                    };
                }
            }
            yield StreamEvent::Done;
        };

        Ok(Box::pin(stream))
    }
}
