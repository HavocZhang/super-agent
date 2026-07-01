use crate::{Tool, mcp_client::McpClient};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct McpBridgeTool {
    client: Arc<Mutex<McpClient>>,
    name: String,
    description: String,
    input_schema: Value,
}

impl McpBridgeTool {
    pub fn new(
        client: Arc<Mutex<McpClient>>,
        name: String,
        description: String,
        input_schema: Value,
    ) -> Self {
        Self { client, name, description, input_schema }
    }

    pub fn client(&self) -> &Arc<Mutex<McpClient>> {
        &self.client
    }

    pub fn schema(&self) -> Value {
        self.input_schema.clone()
    }
}

#[async_trait]
impl Tool for McpBridgeTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        self.input_schema.clone()
    }

    async fn execute(&self, args: &Value, _working_dir: &str) -> anyhow::Result<String> {
        let mut client = self.client.lock().await;
        let result = client.call_tool(&self.name, args.clone()).await?;

        if let Some(content) = result.get("content").and_then(|c| c.as_array()) {
            let texts: Vec<String> = content
                .iter()
                .filter_map(|item| {
                    if item["type"].as_str() == Some("text") {
                        item["text"].as_str().map(|s| s.to_string())
                    } else {
                        None
                    }
                })
                .collect();
            if texts.is_empty() {
                Ok(serde_json::to_string_pretty(&result)?)
            } else {
                Ok(texts.join("\n"))
            }
        } else if let Some(is_err) = result.get("isError").and_then(|e| e.as_bool()) {
            if is_err {
                Err(anyhow::anyhow!("MCP tool error: {}", serde_json::to_string(&result)?))
            } else {
                Ok(serde_json::to_string_pretty(&result)?)
            }
        } else {
            Ok(serde_json::to_string_pretty(&result)?)
        }
    }
}
