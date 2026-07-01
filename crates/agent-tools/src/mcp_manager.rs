use crate::Tool;
use crate::mcp_bridge::McpBridgeTool;
use crate::mcp_client::McpClient;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

struct McpServerEntry {
    client: Arc<Mutex<McpClient>>,
    tools: Vec<McpBridgeTool>,
}

pub struct McpManager {
    servers: HashMap<String, McpServerEntry>,
}

impl McpManager {
    pub fn new() -> Self {
        Self { servers: HashMap::new() }
    }

    async fn add_client(
        &mut self,
        name: &str,
        client: McpClient,
    ) -> anyhow::Result<()> {
        let client = Arc::new(Mutex::new(client));

        let tool_list = {
            let locked = client.lock().await;
            locked.tools().to_vec()
        };

        let tools: Vec<McpBridgeTool> = tool_list
            .into_iter()
            .map(|t| McpBridgeTool::new(
                client.clone(),
                format!("mcp__{}__{}", name, t.name),
                t.description,
                t.input_schema,
            ))
            .collect();

        self.servers.insert(name.to_string(), McpServerEntry { client, tools });
        Ok(())
    }

    pub async fn add_stdio(
        &mut self,
        name: &str,
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> anyhow::Result<()> {
        let client = McpClient::connect_stdio(command, args, env).await?;
        self.add_client(name, client).await
    }

    pub async fn add_sse(&mut self, name: &str, url: &str) -> anyhow::Result<()> {
        let client = McpClient::connect_sse(url).await?;
        self.add_client(name, client).await
    }

    pub async fn add_http(&mut self, name: &str, url: &str) -> anyhow::Result<()> {
        let client = McpClient::connect_http(url).await?;
        self.add_client(name, client).await
    }

    pub async fn remove(&mut self, name: &str) -> anyhow::Result<()> {
        if let Some(entry) = self.servers.remove(name) {
            let mut client = entry.client.lock().await;
            client.close().await?;
        }
        Ok(())
    }

    pub fn get_all_tools(&self) -> Vec<Box<dyn Tool>> {
        let mut tools: Vec<Box<dyn Tool>> = Vec::new();
        for entry in self.servers.values() {
            for tool in &entry.tools {
                tools.push(Box::new(McpBridgeTool::new(
                    tool.client().clone(),
                    tool.name().to_string(),
                    tool.description().to_string(),
                    tool.schema(),
                )));
            }
        }
        tools
    }

    pub fn list_servers(&self) -> Vec<&str> {
        self.servers.keys().map(|s| s.as_str()).collect()
    }

    pub async fn close_all(&mut self) -> anyhow::Result<()> {
        for (_, entry) in self.servers.drain() {
            let mut client = entry.client.lock().await;
            client.close().await.ok();
        }
        Ok(())
    }
}
