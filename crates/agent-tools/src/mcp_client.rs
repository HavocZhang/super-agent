use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};

#[derive(Debug, Clone)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerInfo {
    pub name: String,
    pub version: String,
}

pub enum McpTransport {
    Stdio {
        child: Child,
        stdin: tokio::process::ChildStdin,
        stdout: BufReader<tokio::process::ChildStdout>,
    },
    Sse {
        url: String,
        client: reqwest::Client,
    },
    Http {
        url: String,
        client: reqwest::Client,
    },
}

pub struct McpClient {
    transport: McpTransport,
    tools: Vec<McpTool>,
    next_id: u64,
    server_info: Option<McpServerInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: u64,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcNotification {
    jsonrpc: String,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
    #[serde(default)]
    data: Option<Value>,
}

impl McpClient {
    pub async fn connect_stdio(command: &str, args: &[String], env: &HashMap<String, String>) -> Result<Self> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let mut child = cmd.spawn().context("Failed to spawn MCP server process")?;
        let stdin = child.stdin.take().context("Failed to get stdin")?;
        let stdout = child.stdout.take().context("Failed to get stdout")?;
        let stdout = BufReader::new(stdout);

        let mut client = Self {
            transport: McpTransport::Stdio { child, stdin, stdout },
            tools: Vec::new(),
            next_id: 1,
            server_info: None,
        };
        client.initialize().await?;
        Ok(client)
    }

    pub async fn connect_sse(url: &str) -> Result<Self> {
        let client = reqwest::Client::new();
        let mut mcp = Self {
            transport: McpTransport::Sse { url: url.to_string(), client },
            tools: Vec::new(),
            next_id: 1,
            server_info: None,
        };
        mcp.initialize().await?;
        Ok(mcp)
    }

    pub async fn connect_http(url: &str) -> Result<Self> {
        let client = reqwest::Client::new();
        let mut mcp = Self {
            transport: McpTransport::Http { url: url.to_string(), client },
            tools: Vec::new(),
            next_id: 1,
            server_info: None,
        };
        mcp.initialize().await?;
        Ok(mcp)
    }

    async fn send_request(&mut self, method: &str, params: Option<Value>) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.to_string(),
            params,
        };

        let response = match &mut self.transport {
            McpTransport::Stdio { stdin, stdout, .. } => {
                let msg = serde_json::to_string(&request)? + "\n";
                stdin.write_all(msg.as_bytes()).await.context("Failed to write to stdin")?;
                stdin.flush().await?;

                let mut line = String::new();
                stdout.read_line(&mut line).await.context("Failed to read from stdout")?;
                serde_json::from_str::<JsonRpcResponse>(&line)?
            }
            McpTransport::Sse { url, client } | McpTransport::Http { url, client } => {
                let resp = client
                    .post(url.as_str())
                    .json(&request)
                    .send()
                    .await
                    .context("Failed to send HTTP request")?;
                resp.json::<JsonRpcResponse>().await?
            }
        };

        if let Some(err) = response.error {
            return Err(anyhow::anyhow!("MCP RPC error {}: {}", err.code, err.message));
        }

        response.result.ok_or_else(|| anyhow::anyhow!("Empty response from MCP server"))
    }

    async fn send_notification(&mut self, method: &str, params: Option<Value>) -> Result<()> {
        let request = JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params,
        };

        match &mut self.transport {
            McpTransport::Stdio { stdin, .. } => {
                let msg = serde_json::to_string(&request)? + "\n";
                stdin.write_all(msg.as_bytes()).await?;
                stdin.flush().await?;
                Ok(())
            }
            McpTransport::Sse { url, client } | McpTransport::Http { url, client } => {
                client.post(url.as_str()).json(&request).send().await?;
                Ok(())
            }
        }
    }

    async fn initialize(&mut self) -> Result<()> {
        let params = serde_json::json!({
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {
                "name": "coding-agent",
                "version": "0.1"
            }
        });

        let result = self.send_request("initialize", Some(params)).await?;

        if let Some(si) = result.get("serverInfo") {
            self.server_info = Some(McpServerInfo {
                name: si["name"].as_str().unwrap_or("unknown").to_string(),
                version: si["version"].as_str().unwrap_or("unknown").to_string(),
            });
        }

        self.send_notification("notifications/initialized", None).await?;

        let tools = self.list_tools().await?;
        self.tools = tools;
        Ok(())
    }

    pub async fn list_tools(&self) -> Result<Vec<McpTool>> {
        let result = self.clone_request("tools/list", None).await?;
        let tools = result
            .get("tools")
            .and_then(|t| t.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|t| McpTool {
                        name: t["name"].as_str().unwrap_or("").to_string(),
                        description: t["description"].as_str().unwrap_or("").to_string(),
                        input_schema: t.get("inputSchema").cloned().unwrap_or(serde_json::json!({})),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(tools)
    }

    pub async fn call_tool(&mut self, name: &str, args: Value) -> Result<Value> {
        let params = serde_json::json!({
            "name": name,
            "arguments": args
        });
        self.send_request("tools/call", Some(params)).await
    }

    pub fn tools(&self) -> &[McpTool] {
        &self.tools
    }

    pub fn server_info(&self) -> Option<&McpServerInfo> {
        self.server_info.as_ref()
    }

    pub async fn close(&mut self) -> Result<()> {
        if let McpTransport::Stdio { child, .. } = &mut self.transport {
            child.kill().await.ok();
        }
        Ok(())
    }

    async fn clone_request(&self, method: &str, params: Option<Value>) -> Result<Value> {
        let id = self.next_id;
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.to_string(),
            params,
        };

        match &self.transport {
            McpTransport::Sse { url, client } | McpTransport::Http { url, client } => {
                let resp = client.post(url).json(&request).send().await?;
                let response: JsonRpcResponse = resp.json().await?;
                if let Some(err) = response.error {
                    return Err(anyhow::anyhow!("MCP RPC error {}: {}", err.code, err.message));
                }
                response.result.ok_or_else(|| anyhow::anyhow!("Empty response"))
            }
            _ => Err(anyhow::anyhow!("clone_request only for HTTP/SSE")),
        }
    }
}
