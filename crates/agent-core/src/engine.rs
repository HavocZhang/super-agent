use crate::{AgentConfig, ContextManager, PermissionManager, PermissionMode, PermissionResult, ToolExecutor};
use agent_llm::{ChatRequest, ChatResponse, LlmProvider, Message, StreamEvent, StreamResponse};
use agent_memory::MemoryStore;
use agent_tools::ToolRegistry;
use anyhow::Result;
use futures_util::StreamExt;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info, warn};

const MAX_TOOL_OUTPUT_BYTES: usize = 32 * 1024; // ~8K tokens

fn truncate_tool_output(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    let mut result = s[..end].to_string();
    result.push_str(&format!("\n[...truncated from {} bytes]", s.len()));
    result
}

pub struct AgentEngine {
    llm: Arc<Box<dyn LlmProvider>>,
    tools: Arc<ToolRegistry>,
    config: AgentConfig,
    messages: Arc<RwLock<Vec<Message>>>,
    working_dir: Arc<RwLock<String>>,
    memory: Option<Arc<MemoryStore>>,
    permissions: Arc<PermissionManager>,
    context: Arc<ContextManager>,
}

impl AgentEngine {
    fn new_impl(llm: Arc<Box<dyn LlmProvider>>, tools: ToolRegistry, config: AgentConfig) -> Self {
        let messages = Arc::new(RwLock::new(vec![Message::system(&config.system_prompt)]));
        let perm_mode = PermissionMode::from_str(&config.permission_mode);
        let context = ContextManager::new(config.context_max_tokens);
        let working_dir = Arc::new(RwLock::new(
            std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".to_string()),
        ));
        Self {
            llm,
            tools: Arc::new(tools),
            config,
            messages,
            working_dir,
            memory: None,
            permissions: Arc::new(PermissionManager::new(perm_mode)),
            context: Arc::new(context),
        }
    }

    pub fn new(llm: Box<dyn LlmProvider>, tools: ToolRegistry, config: AgentConfig) -> Self {
        Self::new_impl(Arc::new(llm), tools, config)
    }

    pub fn from_parts(llm: Arc<Box<dyn LlmProvider>>, tools: ToolRegistry, config: AgentConfig) -> Self {
        Self::new_impl(llm, tools, config)
    }

    pub fn llm_clone(&self) -> Arc<Box<dyn LlmProvider>> {
        Arc::clone(&self.llm)
    }

    pub fn tools_arc(&self) -> Arc<ToolRegistry> {
        Arc::clone(&self.tools)
    }

    pub fn with_memory(mut self, path: &str) -> Result<Self> {
        self.memory = Some(Arc::new(MemoryStore::new(path)?));
        Ok(self)
    }

    pub fn with_memory_store(mut self, store: MemoryStore) -> Self {
        self.memory = Some(Arc::new(store));
        self
    }

    pub async fn set_working_dir(&self, dir: &str) {
        let mut wd = self.working_dir.write().await;
        *wd = dir.to_string();
    }

    pub async fn working_dir(&self) -> String {
        self.working_dir.read().await.clone()
    }

    pub async fn clear(&self) {
        let mut msgs = self.messages.write().await;
        *msgs = vec![Message::system(&self.config.system_prompt)];
    }

    pub async fn messages(&self) -> Vec<Message> {
        self.messages.read().await.clone()
    }

    pub fn config(&self) -> &AgentConfig {
        &self.config
    }

    pub async fn run(&self, user_message: &str) -> Result<String> {
        {
            let mut msgs = self.messages.write().await;
            msgs.push(Message::user(user_message));
        }

        self.inject_memories(user_message).await;

        let tools = self.tools.get_definitions();
        let wd = self.working_dir.read().await.clone();

        for _iteration in 0..self.config.max_iterations {
            let request = {
                let msgs = self.messages.read().await;
                ChatRequest {
                    model: self.config.model.clone(),
                    messages: msgs.clone(),
                    tools: tools.clone(),
                    temperature: self.config.temperature,
                    max_tokens: self.config.max_tokens,
                }
            };

            let response = self.llm.chat(request).await?;

            match response {
                ChatResponse::Text(text) => {
                    {
                        let mut msgs = self.messages.write().await;
                        msgs.push(Message::assistant(&text));
                    }
                    self.extract_and_store_memory(&text);
                    return Ok(text);
                }
                ChatResponse::ToolCall(tool_calls) => {
                    {
                        let mut msgs = self.messages.write().await;
                        msgs.push(Message::assistant_with_tool_calls(tool_calls.clone()));
                    }
                    for tool_call in &tool_calls {
                        self.check_permission(&tool_call.name, &tool_call.arguments)?;
                        let result = self.tools.execute(&tool_call.name, &tool_call.arguments, &wd).await;
                        let output = match result {
                            Ok(output) => truncate_tool_output(&output, MAX_TOOL_OUTPUT_BYTES),
                            Err(e) => format!("Error: {}", e),
                        };
                        let mut msgs = self.messages.write().await;
                        msgs.push(Message::tool_result(&tool_call.id, &output));
                    }
                }
            }
        }

        Err(anyhow::anyhow!("Max iterations ({}) reached", self.config.max_iterations))
    }

    pub async fn run_stream(&self, user_message: &str) -> StreamResponse {
        // Add user message
        {
            let mut msgs = self.messages.write().await;
            msgs.push(Message::user(user_message));
        }

        self.inject_memories(user_message).await;

        // Clone everything needed for the spawned task
        let messages = self.messages.clone();
        let llm = self.llm.clone();
        let tools = self.tools.clone();
        let tools_def = self.tools.get_definitions();
        let wd = self.working_dir.read().await.clone();
        let llm_model = self.config.model.clone();
        let llm_temp = self.config.temperature;
        let llm_max = self.config.max_tokens;
        let max_iter = self.config.max_iterations;
        let permissions = self.permissions.clone();
        let context = self.context.clone();
        let memory = self.memory.clone();

        // Create channel for streaming events
        let (tx, rx) = mpsc::channel::<Result<StreamEvent>>(100);

        // Spawn the agent loop as a background task
        tokio::spawn(async move {
            let mut last_tool_name: Option<String> = None;
            let mut consecutive_same_tool: usize = 0;

            for iteration in 0..max_iter {
                info!("Agent iteration {}/{}", iteration + 1, max_iter);

                // Check context compaction
                {
                    let msgs = messages.read().await;
                    if context.needs_compaction(&msgs) {
                        let compacted = context.compact(&msgs, "[Context was auto-compacted]");
                        let mut msgs = messages.write().await;
                        *msgs = compacted;
                        info!("Context auto-compacted");
                    }
                }

                // Check context overflow
                {
                    let msgs = messages.read().await;
                    if context.is_critical(&msgs) {
                        warn!("Context overflow detected");
                        let _ = tx.send(Ok(StreamEvent::Error("Context overflow: token limit exceeded".to_string()))).await;
                        return;
                    }
                }

                let request = {
                    let msgs = messages.read().await;
                    ChatRequest {
                        model: llm_model.clone(),
                        messages: msgs.clone(),
                        tools: tools_def.clone(),
                        temperature: llm_temp,
                        max_tokens: llm_max,
                    }
                };

                let stream_result = llm.chat_stream(request).await;

                match stream_result {
                    Ok(mut stream) => {
                        let mut full_text = String::new();
                        let mut tool_calls_pending = vec![];

                        while let Some(event) = stream.next().await {
                            match event {
                                Ok(StreamEvent::Token(token)) => {
                                    full_text.push_str(&token);
                                    if tx.send(Ok(StreamEvent::Token(token))).await.is_err() {
                                        return;
                                    }
                                }
                                Ok(StreamEvent::ToolCallStart { id, name }) => {
                                    if tx.send(Ok(StreamEvent::ToolCallStart { id, name })).await.is_err() {
                                        return;
                                    }
                                }
                                Ok(StreamEvent::ToolCallEnd { id, name, arguments }) => {
                                    tool_calls_pending.push(agent_llm::ToolCall {
                                        id: id.clone(),
                                        name: name.clone(),
                                        arguments: arguments.clone(),
                                    });
                                    if tx.send(Ok(StreamEvent::ToolCallEnd { id, name, arguments })).await.is_err() {
                                        return;
                                    }
                                }
                                Ok(StreamEvent::Done) => break,
                                Ok(StreamEvent::Error(e)) => {
                                    let _ = tx.send(Ok(StreamEvent::Error(e))).await;
                                    return;
                                }
                                Ok(_) => {}
                                Err(e) => {
                                    let _ = tx.send(Err(e)).await;
                                    return;
                                }
                            }
                        }

                        // If no tool calls, we're done
                        if tool_calls_pending.is_empty() {
                            if !full_text.is_empty() {
                                let mut msgs = messages.write().await;
                                msgs.push(Message::assistant(&full_text));
                            }
                            extract_and_store_memory_static(&memory, &full_text);
                            let _ = tx.send(Ok(StreamEvent::Done)).await;
                            return;
                        }

                        // Store assistant message with tool calls
                        {
                            let mut msgs = messages.write().await;
                            if full_text.is_empty() {
                                msgs.push(Message::assistant_with_tool_calls(tool_calls_pending.clone()));
                            } else {
                                msgs.push(Message::assistant_with_content_and_tool_calls(&full_text, tool_calls_pending.clone()));
                            }
                        }

                        // Check permissions before executing
                        let mut approved_calls: Vec<agent_llm::ToolCall> = Vec::new();
                        for tc in &tool_calls_pending {
                            match permissions.check(&tc.name, &tc.arguments) {
                                PermissionResult::Allowed => {
                                    approved_calls.push(tc.clone());
                                }
                                PermissionResult::Denied(reason) => {
                                    let mut msgs = messages.write().await;
                                    msgs.push(Message::tool_result(&tc.id, &format!("Permission denied: {}", reason)));
                                }
                                PermissionResult::NeedsApproval(msg) => {
                                    // In Default mode, tools requiring approval are allowed with a warning
                                    // In stricter modes, this should be denied or asked
                                    warn!("Tool '{}' requires approval but auto-approved: {}", tc.name, msg);
                                    approved_calls.push(tc.clone());
                                }
                            }
                        }

                        // Execute approved tools (parallel when possible)
                        let tool_executor = ToolExecutor::new(Arc::clone(&tools));

                        // Send snapshots BEFORE executing any tools (for diff computation)
                        for tc in &approved_calls {
                            if tc.name == "file_write" || tc.name == "file_edit" {
                                if let Some(path) = tc.arguments.get("path").and_then(|v| v.as_str()) {
                                    let snapshot = tokio::fs::read_to_string(path).await.unwrap_or_default();
                                    let _ = tx.send(Ok(StreamEvent::ToolSnapshot {
                                        path: path.to_string(),
                                        content: snapshot,
                                    })).await;
                                }
                            }
                        }

                        let results = tool_executor.execute_batch(&approved_calls, &wd).await;
                        for result in results {
                            let output = truncate_tool_output(&result.output, MAX_TOOL_OUTPUT_BYTES);
                            info!("Tool {} done ({} bytes)", result.name, output.len());

                            // Doom loop detection
                            if last_tool_name.as_deref() == Some(&result.name) {
                                consecutive_same_tool += 1;
                            } else {
                                last_tool_name = Some(result.name.clone());
                                consecutive_same_tool = 1;
                            }
                            if consecutive_same_tool >= 3 {
                                warn!("Doom loop detected: tool {} called 3 times consecutively", result.name);
                                let _ = tx.send(Ok(StreamEvent::Error(
                                    format!("Doom loop detected: tool {} called 3 times consecutively", result.name)
                                ))).await;
                                return;
                            }

                            let mut msgs = messages.write().await;
                            msgs.push(Message::tool_result(&result.tool_call_id, &output));
                        }

                        info!("Tools done, continuing to next iteration");
                    }
                    Err(e) => {
                        warn!("LLM stream error: {}", e);
                        let _ = tx.send(Err(e)).await;
                        return;
                    }
                }
            }

            // Max iterations reached
            warn!("Max iterations ({}) reached", max_iter);
            let _ = tx.send(Ok(StreamEvent::Done)).await;
        });

        // Return the receiver as a stream
        Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx))
    }

    async fn inject_memories(&self, user_message: &str) {
        if let Some(memory) = &self.memory {
            let memories = memory.search(user_message, 5);
            let mut msgs = self.messages.write().await;
            msgs.retain(|m| {
                !(m.role == agent_llm::Role::System && m.content.starts_with("Relevant memories:\n"))
            });
            if !memories.is_empty() {
                let memory_context: String = memories
                    .iter()
                    .map(|m| format!("- {}", m.content))
                    .collect::<Vec<_>>()
                    .join("\n");
                msgs.push(Message::system(&format!("Relevant memories:\n{}", memory_context)));
                debug!("Injected {} memories into context", memories.len());
            }
        }
    }

    fn extract_and_store_memory(&self, response: &str) {
        extract_and_store_memory_static(&self.memory, response);
    }

    fn check_permission(&self, tool_name: &str, args: &serde_json::Value) -> Result<()> {
        match self.permissions.check(tool_name, args) {
            PermissionResult::Allowed => Ok(()),
            PermissionResult::Denied(reason) => Err(anyhow::anyhow!("Permission denied: {}", reason)),
            PermissionResult::NeedsApproval(msg) => {
                warn!("Tool '{tool_name}' requires approval but auto-approved in stream mode: {msg}");
                Ok(())
            }
        }
    }
}

fn extract_and_store_memory_static(memory: &Option<Arc<MemoryStore>>, response: &str) {
    let Some(memory) = memory else { return; };

    let keywords = ["remember", "important", "note:", "todo:", "fix:", "bug:"];
    let has_keyword = keywords.iter().any(|kw| response.to_lowercase().contains(kw));

    if has_keyword {
        let content: String = response.chars().take(500).collect();
        if let Err(e) = memory.store(&content, "extracted", 0.6) {
            debug!("Failed to store memory: {}", e);
        }
    }
}
