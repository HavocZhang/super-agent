use crate::{AgentConfig, ContextManager, PermissionManager, PermissionMode, PermissionResult, ToolExecutor};
use agent_llm::{ChatRequest, ChatResponse, LlmProvider, Message, StreamEvent, StreamResponse};
use agent_memory::MemoryStore;
use agent_tools::ToolRegistry;
use anyhow::Result;
use futures_util::StreamExt;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info, warn};

// ── Agent 引擎 ──────────────────────────────────────────────
// 核心运行时，管理消息循环、工具调用、权限检查、上下文窗口等

/// 工具输出最大字节数（约 4K tokens）
const MAX_TOOL_OUTPUT_BYTES: usize = 16 * 1024;

/// 每轮工具输出总字节数上限（约 50K tokens）
/// 超过此值后强制压缩上下文
const MAX_TURN_TOOL_OUTPUT_BYTES: usize = 200 * 1024;

/// 截断工具输出到指定字节数，确保保持在上下文窗口内
/// 会正确处理 UTF-8 字符边界
pub fn truncate_tool_output(s: &str, max_bytes: usize) -> String {
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

/// Agent 引擎 —— 核心运行时
///
/// 管理完整的 Agent 循环:
/// 1. 接收用户消息
/// 2. 注入相关记忆
/// 3. 调用 LLM 获取响应
/// 4. 执行工具调用（支持并行）
/// 5. 检测死循环（doom loop）
/// 6. 上下文压缩
pub struct AgentEngine {
    /// LLM 提供者（如 OpenAI、DeepSeek）
    llm: Arc<Box<dyn LlmProvider>>,
    /// 工具注册表
    tools: Arc<ToolRegistry>,
    /// Agent 配置
    config: AgentConfig,
    /// 对话消息列表（线程安全）
    messages: Arc<RwLock<Vec<Message>>>,
    /// 当前工作目录
    working_dir: Arc<RwLock<String>>,
    /// 可选的记忆存储
    memory: Option<Arc<MemoryStore>>,
    /// 权限管理器
    permissions: Arc<PermissionManager>,
    /// 上下文管理器（token 估算、压缩、溢出检测）
    context: Arc<ContextManager>,
}

impl AgentEngine {
    /// 内部构造函数，统一处理 Arc 包装
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

    /// 创建新的 Agent 引擎
    pub fn new(llm: Box<dyn LlmProvider>, tools: ToolRegistry, config: AgentConfig) -> Self {
        Self::new_impl(Arc::new(llm), tools, config)
    }

    /// 从已 Arc 包装的 LLM 提供者创建
    pub fn from_parts(llm: Arc<Box<dyn LlmProvider>>, tools: ToolRegistry, config: AgentConfig) -> Self {
        Self::new_impl(llm, tools, config)
    }

    /// 克隆 LLM 提供者的 Arc 引用（用于子 agent）
    pub fn llm_clone(&self) -> Arc<Box<dyn LlmProvider>> {
        Arc::clone(&self.llm)
    }

    /// 获取工具注册表的 Arc 引用
    pub fn tools_arc(&self) -> Arc<ToolRegistry> {
        Arc::clone(&self.tools)
    }

    /// 启用记忆存储（从文件路径）
    pub fn with_memory(mut self, path: &str) -> Result<Self> {
        self.memory = Some(Arc::new(MemoryStore::new(path)?));
        Ok(self)
    }

    /// 启用记忆存储（使用已有实例）
    pub fn with_memory_store(mut self, store: MemoryStore) -> Self {
        self.memory = Some(Arc::new(store));
        self
    }

    /// 设置工作目录
    pub async fn set_working_dir(&self, dir: &str) {
        let mut wd = self.working_dir.write().await;
        *wd = dir.to_string();
    }

    /// 获取当前工作目录
    pub async fn working_dir(&self) -> String {
        self.working_dir.read().await.clone()
    }

    /// 清空对话历史（保留系统提示词）
    pub async fn clear(&self) {
        let mut msgs = self.messages.write().await;
        *msgs = vec![Message::system(&self.config.system_prompt)];
    }

    /// 获取所有消息
    pub async fn messages(&self) -> Vec<Message> {
        self.messages.read().await.clone()
    }

    /// 获取配置引用
    pub fn config(&self) -> &AgentConfig {
        &self.config
    }

    /// 运行 Agent（非流式模式）
    ///
    /// 处理用户消息，在 max_iterations 范围内循环：
    /// - 如果 LLM 返回文本 -> 返回结果
    /// - 如果 LLM 调用工具 -> 执行工具 -> 继续循环
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

// ── 流式 Agent 循环 ────────────────────────────────────────
//
// `run_stream` 是 agent-core 的核心功能：
// 1. 接收用户消息，注入记忆
// 2. 在后台任务中运行迭代式 agent 循环
// 3. 通过 mpsc channel 将事件流式发送给 UI
// 4. 支持：上下文压缩、权限检查、死循环检测、并行工具执行

    /// 运行 Agent（流式模式），返回事件流
    ///
    /// 事件类型包括：
    /// - `StreamEvent::Token` - LLM 生成的文本 token
    /// - `StreamEvent::ToolCallStart/ToolCallEnd` - 工具调用
    /// - `StreamEvent::ToolSnapshot` - 文件编辑前的快照
    /// - `StreamEvent::ToolResult` - 工具执行结果
    /// - `StreamEvent::Done` - Agent 完成
    /// - `StreamEvent::Error` - 错误信息
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
            let mut last_call_signature: Option<String> = None;
            let mut consecutive_same_tool: usize = 0;
            let mut turn_tool_bytes: usize = 0;

            for iteration in 0..max_iter {
                info!("Agent iteration {}/{}", iteration + 1, max_iter);

                // Force compaction if accumulated tool output is too large
                if turn_tool_bytes > MAX_TURN_TOOL_OUTPUT_BYTES {
                    warn!("Turn tool output exceeded {}KB, forcing compaction", turn_tool_bytes / 1024);
                    let msgs = messages.read().await;
                    let compacted = context.compact(&msgs, "[Auto-compacted: accumulated tool output exceeded limit]");
                    drop(msgs);
                    let mut msgs = messages.write().await;
                    *msgs = compacted;
                    turn_tool_bytes = 0;
                    info!("Forced compaction done");
                }

                // Check context compaction
                {
                    let msgs = messages.read().await;
                    if context.needs_compaction(&msgs) {
                        let compacted = context.compact(&msgs, "[Context was auto-compacted]");
                        let mut msgs = messages.write().await;
                        *msgs = compacted;
                        turn_tool_bytes = 0;
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

                // LLM call with timeout to prevent hanging
                let stream_result = tokio::time::timeout(
                    std::time::Duration::from_secs(120),
                    llm.chat_stream(request),
                ).await;

                let stream_result = match stream_result {
                    Ok(result) => result,
                    Err(_) => {
                        warn!("LLM call timed out after 120s");
                        let _ = tx.send(Ok(StreamEvent::Error(
                            "LLM call timed out after 120 seconds".to_string()
                        ))).await;
                        return;
                    }
                };

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
                        let mut doom_loop_hit = false;
                        let mut last_tool_call_id = String::new();
                        for result in results {
                            let output = truncate_tool_output(&result.output, MAX_TOOL_OUTPUT_BYTES);
                            turn_tool_bytes += output.len();
                            info!("Tool {} done ({} bytes, turn total {}KB)", result.name, output.len(), turn_tool_bytes / 1024);

                            // Doom loop detection
                            let call_signature = format!("{}:{}", result.name, result.arguments_hash);
                            if last_call_signature.as_deref() == Some(&call_signature) {
                                consecutive_same_tool += 1;
                            } else {
                                last_call_signature = Some(call_signature);
                                consecutive_same_tool = 1;
                            }

                            // Send tool result to TUI
                            let _ = tx.send(Ok(StreamEvent::ToolResult {
                                id: result.tool_call_id.clone(),
                                name: result.name.clone(),
                                output: output.clone(),
                            })).await;

                            {
                                let mut msgs = messages.write().await;
                                msgs.push(Message::tool_result(&result.tool_call_id, &output));
                            }

                            if consecutive_same_tool >= 3 {
                                warn!("Doom loop detected: tool {} called 3 times consecutively", result.name);
                                doom_loop_hit = true;
                                last_tool_call_id = result.tool_call_id.clone();
                                let _ = tx.send(Ok(StreamEvent::Error(
                                    format!("Doom loop detected: tool {} called 3 times consecutively", result.name)
                                ))).await;
                                break;
                            }
                        }

                        if doom_loop_hit {
                            // Backfill tool_results for unexecuted tool_calls
                            let unexecuted: Vec<String> = tool_calls_pending
                                .iter()
                                .skip_while(|tc| tc.id != last_tool_call_id)
                                .skip(1)
                                .map(|tc| tc.id.clone())
                                .collect();
                            if !unexecuted.is_empty() {
                                let mut msgs = messages.write().await;
                                for tc_id in unexecuted {
                                    msgs.push(Message::tool_result(
                                        &tc_id,
                                        "Tool execution skipped due to doom loop detection",
                                    ));
                                }
                            }
                            return;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_short_output() {
        let output = "short";
        let result = truncate_tool_output(output, 100);
        assert_eq!(result, "short");
    }

    #[test]
    fn test_truncate_long_output() {
        let output = "a".repeat(50000);
        let result = truncate_tool_output(&output, 32 * 1024);
        assert!(result.len() < 50000);
        assert!(result.contains("truncated"));
    }

    #[test]
    fn test_truncate_exact_boundary() {
        let output = "a".repeat(32 * 1024);
        let result = truncate_tool_output(&output, 32 * 1024);
        assert_eq!(result, output);
    }

    #[test]
    fn test_truncate_utf8_boundary() {
        let output = "你好世界".repeat(20000);
        let result = truncate_tool_output(&output, 32 * 1024);
        assert!(result.len() <= 32 * 1024 + 100);
        assert!(result.contains("truncated"));
    }
}
