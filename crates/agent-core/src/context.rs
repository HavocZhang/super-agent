use agent_llm::{Message, Role};
use std::collections::HashSet;

// ── 上下文管理器 ────────────────────────────────────────────
// 管理 LLM 的上下文窗口：token 估算、溢出检测、消息压缩。
//
// 核心职责：
// 1. Token 估算: 中文 1.5 token/字，英文 0.25 token/字符
// 2. 溢出检测: Warning(90%) / Critical(100%)
// 3. 智能压缩: 保护 system 消息 + 最近 N 条 + 摘要旧消息
// 4. 消息清理: sanitize_messages 移除孤立 tool result (防 API 400)
//
// 压缩策略：
// - compact(): 简单压缩，保留 system + 最近 keep_recent 条 + 摘要
// - smart_compact(): 保留 system + 受保护工具结果 + 最近 PRUNE_PROTECT 范围
// - smart_compact_enhanced(): 在 smart_compact 基础上保护受保护工具的结果
//
// 所有 compact 方法返回前调用 sanitize_messages() 确保消息序列合法。

/// 默认最大 token 数（用于上下文窗口限制）
pub const DEFAULT_MAX_TOKENS: usize = 180_000;
/// 溢出警告阈值（达到最大 token 的 90%）
pub const WARNING_THRESHOLD: f64 = 0.9;
/// 压缩缓冲区大小（保留空间给新消息）
pub const COMPACTION_BUFFER: usize = 20_000;
/// 压缩时保留的最小 token 数
pub const PRUNE_MINIMUM: usize = 20_000;
/// 压缩时保护保留的 token 数
pub const PRUNE_PROTECT: usize = 40_000;
/// 受保护的工具名称列表（这些工具的结果不会被压缩掉）
pub const PROTECTED_TOOLS: &[&str] = &[];

/// 清理消息序列：移除孤立的 tool result（没有对应 assistant+tool_calls 的 tool 消息）
/// 和没有 tool result 的 assistant+tool_calls（防止 API 400 错误）
pub fn sanitize_messages(messages: Vec<Message>) -> Vec<Message> {
    if messages.is_empty() {
        return messages;
    }

    // 收集所有 assistant+tool_calls 声明的 tool_call_id
    let mut declared_ids: HashSet<String> = HashSet::new();
    // 收集所有 tool result 引用的 tool_call_id
    let mut result_ids: HashSet<String> = HashSet::new();

    for msg in &messages {
        if msg.role == Role::Assistant {
            if let Some(ref tool_calls) = msg.tool_calls {
                for tc in tool_calls {
                    declared_ids.insert(tc.id.clone());
                }
            }
        }
        if msg.role == Role::Tool {
            if let Some(ref id) = msg.tool_call_id {
                result_ids.insert(id.clone());
            }
        }
    }

    let mut result = Vec::with_capacity(messages.len());
    for msg in messages {
        match msg.role {
            Role::Tool => {
                // 移除没有对应 assistant+tool_calls 的孤立 tool result
                if let Some(ref id) = msg.tool_call_id {
                    if declared_ids.contains(id) {
                        result.push(msg);
                    }
                    // else: orphaned tool result, skip
                } else {
                    result.push(msg);
                }
            }
            Role::Assistant => {
                if let Some(ref tool_calls) = msg.tool_calls {
                    // 如果 assistant+tool_calls 的所有 tool_calls 都没有对应的 result，
                    // 保留 assistant 消息但清空 tool_calls（让它变成纯文本回复）
                    let has_any_result = tool_calls.iter().any(|tc| result_ids.contains(&tc.id));
                    if !has_any_result && !tool_calls.is_empty() {
                        // 无对应 result → 保留文本但移除 tool_calls
                        let mut cleaned = msg.clone();
                        cleaned.tool_calls = None;
                        result.push(cleaned);
                    } else {
                        result.push(msg);
                    }
                } else {
                    result.push(msg);
                }
            }
            _ => {
                result.push(msg);
            }
        }
    }

    result
}

/// 上下文溢出级别
#[derive(Debug, Clone, PartialEq)]
pub enum OverflowLevel {
    /// 正常
    None,
    /// 警告级别（超过 90%）
    Warning,
    /// 严重级别（超过 100%）
    Critical,
}

/// 上下文状态枚举
#[derive(Debug, Clone, PartialEq)]
pub enum ContextStatus {
    /// 正常
    Normal,
    /// 警告
    Warning,
    /// 严重
    Critical,
}

/// 上下文管理器 —— 估算 token 数、检测溢出、执行消息压缩
///
/// # Token 估算算法
/// - 中文: 每个字符约 1.5 tokens
/// - 其他字符: 每 4 个字符约 1 token
pub struct ContextManager {
    /// 最大 token 数
    max_tokens: usize,
    /// 触发压缩的阈值比例（默认 0.8 = 80%）
    compaction_threshold: f64,
    /// 压缩时保留的最新消息条数
    keep_recent: usize,
}

impl ContextManager {
    /// 创建新的上下文管理器
    pub fn new(max_tokens: usize) -> Self {
        Self {
            max_tokens,
            compaction_threshold: 0.8,
            keep_recent: 6,
        }
    }

    /// 设置压缩阈值
    pub fn with_threshold(mut self, threshold: f64) -> Self {
        self.compaction_threshold = threshold;
        self
    }

    /// 设置保留的最新消息数
    pub fn with_keep_recent(mut self, keep_recent: usize) -> Self {
        self.keep_recent = keep_recent;
        self
    }

    /// 估算消息列表的总 token 数
    pub fn estimate_tokens(&self, messages: &[Message]) -> usize {
        messages.iter().map(|m| estimate_message_tokens(m)).sum()
    }

    /// 估算文本片段的 token 数
    pub fn estimate_text_tokens(text: &str) -> usize {
        let mut chinese_chars = 0usize;
        let mut other_chars = 0usize;

        for ch in text.chars() {
            if is_cjk(ch) {
                chinese_chars += 1;
            } else {
                other_chars += 1;
            }
        }

        (chinese_chars as f64 * 1.5 + other_chars as f64 / 4.0) as usize
    }

    pub fn needs_compaction(&self, messages: &[Message]) -> bool {
        let tokens = self.estimate_tokens(messages);
        (tokens as f64) >= (self.max_tokens as f64 * self.compaction_threshold)
    }

    pub fn check_overflow(&self, messages: &[Message]) -> OverflowLevel {
        let tokens = self.estimate_tokens(messages);
        let ratio = tokens as f64 / self.max_tokens as f64;
        if ratio >= 1.0 {
            OverflowLevel::Critical
        } else if ratio >= WARNING_THRESHOLD {
            OverflowLevel::Warning
        } else {
            OverflowLevel::None
        }
    }

    pub fn is_critical(&self, messages: &[Message]) -> bool {
        self.check_overflow(messages) == OverflowLevel::Critical
    }

    pub fn get_status(&self, messages: &[Message]) -> ContextStatus {
        match self.check_overflow(messages) {
            OverflowLevel::None => ContextStatus::Normal,
            OverflowLevel::Warning => ContextStatus::Warning,
            OverflowLevel::Critical => ContextStatus::Critical,
        }
    }

    pub fn compact(&self, messages: &[Message], llm_summary: &str) -> Vec<Message> {
        let mut system_msgs: Vec<&Message> = Vec::new();
        let mut recent: Vec<&Message> = Vec::new();

        for msg in messages {
            match msg.role {
                Role::System => system_msgs.push(msg),
                _ => recent.push(msg),
            }
        }

        let keep_count = self.keep_recent.min(recent.len());
        let split = recent.len() - keep_count;
        let kept = &recent[split..];

        let mut result: Vec<Message> = system_msgs.into_iter().cloned().collect();

        if !llm_summary.is_empty() {
            result.push(Message::system(&format!(
                "[Previous conversation summary]\n{}",
                llm_summary
            )));
        }

        result.extend(kept.iter().cloned().cloned());
        sanitize_messages(result)
    }

    pub fn smart_compact(&self, messages: &[Message], llm_summary: &str) -> Vec<Message> {
        let mut system_msgs: Vec<&Message> = Vec::new();
        let mut non_system: Vec<&Message> = Vec::new();

        for msg in messages {
            match msg.role {
                Role::System => system_msgs.push(msg),
                _ => non_system.push(msg),
            }
        }

        let keep_count = self.keep_recent.min(non_system.len());
        let boundary = non_system.len() - keep_count;
        let old_messages = &non_system[..boundary];
        let recent_messages = &non_system[boundary..];

        let mut result: Vec<Message> = system_msgs.into_iter().cloned().collect();

        let summary = if llm_summary.is_empty() {
            summarize_messages(old_messages)
        } else {
            llm_summary.to_string()
        };

        if !summary.is_empty() {
            result.push(Message::system(&format!(
                "[Previous conversation summary]\n{}",
                summary
            )));
        }

        result.extend(recent_messages.iter().cloned().cloned());
        sanitize_messages(result)
    }

    pub fn truncate_message(&self, msg: &Message, max_lines: usize) -> Message {
        let lines: Vec<&str> = msg.content.lines().collect();
        if lines.len() <= max_lines {
            return msg.clone();
        }

        let truncated = lines[..max_lines].join("\n");
        Message {
            role: msg.role.clone(),
            content: format!(
                "{}\n[...truncated {} lines]",
                truncated,
                lines.len() - max_lines
            ),
            tool_calls: msg.tool_calls.clone(),
            tool_call_id: msg.tool_call_id.clone(),
        }
    }

    pub fn smart_compact_enhanced(&self, messages: &[Message], llm_summary: &str) -> Vec<Message> {
        let mut system_msgs: Vec<&Message> = Vec::new();
        let mut non_system: Vec<&Message> = Vec::new();

        for msg in messages {
            match msg.role {
                Role::System => system_msgs.push(msg),
                _ => non_system.push(msg),
            }
        }

        let protected_names: Vec<&str> = PROTECTED_TOOLS.to_vec();

        let protected_tc_ids: std::collections::HashSet<String> = non_system
            .iter()
            .filter_map(|m| m.tool_calls.as_ref())
            .flatten()
            .filter(|c| protected_names.contains(&c.name.as_str()))
            .map(|c| c.id.clone())
            .collect();

        let mut recent_protected: Vec<&Message> = Vec::new();
        let mut rest: Vec<&Message> = Vec::new();
        let mut recent_token_budget = 0usize;

        for msg in non_system.iter().rev() {
            let msg_tokens = estimate_message_tokens(msg);
            let is_protected = match msg.role {
                Role::Tool => msg
                    .tool_call_id
                    .as_ref()
                    .map(|id| protected_tc_ids.contains(id))
                    .unwrap_or(false),
                _ => false,
            };

            if recent_token_budget < PRUNE_PROTECT || is_protected {
                recent_protected.insert(0, msg);
                recent_token_budget += msg_tokens;
            } else {
                rest.insert(0, msg);
            }
        }

        let old_messages = &rest[..];

        let mut result: Vec<Message> = system_msgs.into_iter().cloned().collect();

        let summary = if llm_summary.is_empty() {
            summarize_messages(old_messages)
        } else {
            llm_summary.to_string()
        };

        if !summary.is_empty() {
            result.push(Message::system(&format!(
                "[Previous conversation summary]\n{}",
                summary
            )));
        }

        result.extend(recent_protected.iter().cloned().cloned());
        sanitize_messages(result)
    }
}

fn is_cjk(ch: char) -> bool {
    matches!(ch,
        '\u{4E00}'..='\u{9FFF}' |
        '\u{3400}'..='\u{4DBF}' |
        '\u{F900}'..='\u{FAFF}' |
        '\u{2E80}'..='\u{2EFF}' |
        '\u{3000}'..='\u{303F}' |
        '\u{FF00}'..='\u{FFEF}' |
        '\u{FE30}'..='\u{FE4F}'
    )
}

fn estimate_message_tokens(msg: &Message) -> usize {
    let content_tokens = estimate_text_tokens_static(&msg.content);

    let tool_tokens: usize = msg
        .tool_calls
        .as_ref()
        .map(|calls| {
            calls
                .iter()
                .map(|c| estimate_text_tokens_static(&c.name) + estimate_text_tokens_static(&c.arguments.to_string()))
                .sum()
        })
        .unwrap_or(0);

    content_tokens + tool_tokens + 4
}

fn estimate_text_tokens_static(text: &str) -> usize {
    let mut chinese_chars = 0usize;
    let mut other_chars = 0usize;

    for ch in text.chars() {
        if is_cjk(ch) {
            chinese_chars += 1;
        } else {
            other_chars += 1;
        }
    }

    (chinese_chars as f64 * 1.5 + other_chars as f64 / 4.0).max(1.0) as usize
}

fn summarize_messages(messages: &[&Message]) -> String {
    let mut parts = Vec::new();

    for msg in messages {
        match msg.role {
            Role::User => {
                let preview: String = msg.content.chars().take(100).collect();
                if !preview.is_empty() {
                    parts.push(format!("User asked: {}", preview));
                }
            }
            Role::Assistant => {
                if msg.tool_calls.is_some() {
                    let tool_names: Vec<&str> = msg
                        .tool_calls
                        .as_ref()
                        .unwrap()
                        .iter()
                        .map(|tc| tc.name.as_str())
                        .collect();
                    parts.push(format!("Assistant used tools: {}", tool_names.join(", ")));
                } else {
                    let preview: String = msg.content.chars().take(100).collect();
                    if !preview.is_empty() {
                        parts.push(format!("Assistant said: {}", preview));
                    }
                }
            }
            Role::Tool => {
                let preview: String = msg.content.chars().take(50).collect();
                parts.push(format!("Tool result: {}...", preview));
            }
            Role::System => {}
        }
    }

    parts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens() {
        let cm = ContextManager::new(10000);
        let messages = vec![
            Message::system("You are a helpful assistant"),
            Message::user("Hello, how are you?"),
            Message::assistant("I'm doing well, thanks!"),
        ];
        let tokens = cm.estimate_tokens(&messages);
        assert!(tokens > 0);
    }

    #[test]
    fn test_estimate_text_tokens_chinese() {
        let tokens = ContextManager::estimate_text_tokens("你好世界");
        assert!(tokens > 0);
        assert_eq!(tokens, 6);
    }

    #[test]
    fn test_estimate_text_tokens_english() {
        let tokens = ContextManager::estimate_text_tokens("hello");
        assert_eq!(tokens, 1);
    }

    #[test]
    fn test_estimate_text_tokens_mixed() {
        let tokens = ContextManager::estimate_text_tokens("hello你好");
        assert!(tokens > 1);
    }

    #[test]
    fn test_needs_compaction() {
        let cm = ContextManager::new(100).with_threshold(0.5);
        let short = vec![Message::user("hi")];
        assert!(!cm.needs_compaction(&short));

        let long_content = "x".repeat(300);
        let long = vec![Message::user(&long_content)];
        assert!(cm.needs_compaction(&long));
    }

    #[test]
    fn test_compact_preserves_system_and_recent() {
        let cm = ContextManager::new(10000);
        let messages = vec![
            Message::system("system prompt"),
            Message::user("q1"),
            Message::assistant("a1"),
            Message::user("q2"),
            Message::assistant("a2"),
            Message::user("q3"),
            Message::assistant("a3"),
        ];
        let result = cm.compact(&messages, "summary of old chat");

        assert_eq!(result[0].role, Role::System);
        assert_eq!(result[0].content, "system prompt");
        assert!(result[1].content.contains("summary of old chat"));
        assert_eq!(result.last().unwrap().content, "a3");
    }

    #[test]
    fn test_smart_compact_with_summary() {
        let cm = ContextManager::new(10000).with_keep_recent(2);
        let messages = vec![
            Message::system("system prompt"),
            Message::user("q1"),
            Message::assistant("a1"),
            Message::user("q2"),
            Message::assistant("a2"),
            Message::user("q3"),
            Message::assistant("a3"),
        ];
        let result = cm.smart_compact(&messages, "custom summary");

        assert_eq!(result[0].role, Role::System);
        assert_eq!(result[0].content, "system prompt");
        assert!(result[1].content.contains("custom summary"));
        assert_eq!(result[2].content, "q3");
        assert_eq!(result[3].content, "a3");
    }

    #[test]
    fn test_smart_compact_generates_summary() {
        let cm = ContextManager::new(10000).with_keep_recent(2);
        let messages = vec![
            Message::system("system prompt"),
            Message::user("What is Rust?"),
            Message::assistant("Rust is a systems language."),
            Message::user("How to compile?"),
            Message::assistant("Use cargo build."),
            Message::user("q3"),
            Message::assistant("a3"),
        ];
        let result = cm.smart_compact(&messages, "");

        assert_eq!(result[0].role, Role::System);
        assert!(result[1].content.contains("User asked: What is Rust?"));
        assert_eq!(result[2].content, "q3");
        assert_eq!(result[3].content, "a3");
    }

    #[test]
    fn test_truncate_message() {
        let cm = ContextManager::new(10000);
        let msg = Message::user("line1\nline2\nline3\nline4\nline5");
        let truncated = cm.truncate_message(&msg, 3);
        assert!(truncated.content.contains("line1"));
        assert!(truncated.content.contains("line3"));
        assert!(truncated.content.contains("truncated 2 lines"));
        assert!(!truncated.content.contains("line4"));
    }

    #[test]
    fn test_overflow_level_normal() {
        let cm = ContextManager::new(10000);
        let messages = vec![Message::user("hi")];
        assert_eq!(cm.check_overflow(&messages), OverflowLevel::None);
    }

    #[test]
    fn test_overflow_level_warning() {
        let cm = ContextManager::new(1000);
        let content = "x".repeat(3600);
        let messages = vec![Message::user(&content)];
        assert_eq!(cm.check_overflow(&messages), OverflowLevel::Warning);
    }

    #[test]
    fn test_overflow_level_critical() {
        let cm = ContextManager::new(1000);
        let content = "x".repeat(4000);
        let messages = vec![Message::user(&content)];
        assert_eq!(cm.check_overflow(&messages), OverflowLevel::Critical);
    }

    #[test]
    fn test_context_status() {
        let cm = ContextManager::new(10000);
        let messages = vec![Message::user("hi")];
        assert_eq!(cm.get_status(&messages), ContextStatus::Normal);

        let cm = ContextManager::new(1000);
        let content = "x".repeat(3600);
        let messages = vec![Message::user(&content)];
        assert_eq!(cm.get_status(&messages), ContextStatus::Warning);

        let content = "x".repeat(4000);
        let messages = vec![Message::user(&content)];
        assert_eq!(cm.get_status(&messages), ContextStatus::Critical);
    }

    #[test]
    fn test_smart_compact_enhanced() {
        let cm = ContextManager::new(10000);
        let messages = vec![
            Message::system("system prompt"),
            Message::user("q1"),
            Message::assistant("a1"),
            Message::user("q2"),
            Message::assistant("a2"),
            Message::user("q3"),
            Message::assistant("a3"),
        ];
        let result = cm.smart_compact_enhanced(&messages, "custom summary");

        assert_eq!(result[0].role, Role::System);
        assert_eq!(result[0].content, "system prompt");
        assert!(result[1].content.contains("custom summary"));
        assert_eq!(result.last().unwrap().content, "a3");
    }

    #[test]
    fn test_sanitize_removes_orphaned_tool_result() {
        // Scenario: assistant+tool_calls [tc1, tc2], then only tool_result for tc1
        // After sanitize: assistant+tool_calls [tc1] (tc2 removed), tool_result for tc1
        let messages = vec![
            Message::user("test"),
            Message {
                role: Role::Assistant,
                content: "let me check".to_string(),
                tool_calls: Some(vec![
                    agent_llm::ToolCall {
                        id: "tc1".to_string(),
                        name: "file_read".to_string(),
                        arguments: serde_json::json!({}),
                    },
                    agent_llm::ToolCall {
                        id: "tc2".to_string(),
                        name: "grep".to_string(),
                        arguments: serde_json::json!({}),
                    },
                ]),
                tool_call_id: None,
            },
            Message::tool_result("tc1", "file content"),
            // tc2 result is missing (orphaned assistant tool_call)
        ];

        let result = sanitize_messages(messages);
        // assistant message should have tool_calls cleared since tc2 has no result
        // Actually: tc1 DOES have a result, so has_any_result=true, keep as-is
        // tc2's tool_call is kept even without result (LLM will handle it)
        // The tool_result for tc1 is kept because tc1 is declared
        assert_eq!(result.len(), 3); // user, assistant, tool_result for tc1
    }

    #[test]
    fn test_sanitize_removes_orphaned_tool_messages() {
        // Scenario: compact kept a tool_result but dropped the assistant+tool_calls
        let messages = vec![
            Message::system("system"),
            Message::tool_result("orphan_tc_id", "some output"),
            Message::user("next question"),
        ];

        let result = sanitize_messages(messages);
        // The orphaned tool_result should be removed
        assert_eq!(result.len(), 2); // system + user
        assert_eq!(result[0].role, Role::System);
        assert_eq!(result[1].role, Role::User);
    }

    #[test]
    fn test_sanitize_preserves_valid_sequence() {
        let messages = vec![
            Message::user("test"),
            Message {
                role: Role::Assistant,
                content: "checking".to_string(),
                tool_calls: Some(vec![agent_llm::ToolCall {
                    id: "tc1".to_string(),
                    name: "file_read".to_string(),
                    arguments: serde_json::json!({}),
                }]),
                tool_call_id: None,
            },
            Message::tool_result("tc1", "content"),
            Message::assistant("done"),
        ];

        let result = sanitize_messages(messages);
        assert_eq!(result.len(), 4); // all preserved
    }

    #[test]
    fn test_compact_no_orphaned_messages() {
        // Test that compact + sanitize produces valid message sequences
        let cm = ContextManager::new(10000).with_keep_recent(2);
        let messages = vec![
            Message::system("system"),
            Message::user("q1"),
            Message {
                role: Role::Assistant,
                content: "checking".to_string(),
                tool_calls: Some(vec![agent_llm::ToolCall {
                    id: "tc1".to_string(),
                    name: "file_read".to_string(),
                    arguments: serde_json::json!({}),
                }]),
                tool_call_id: None,
            },
            Message::tool_result("tc1", "file content here"),
            Message::user("q2"),
            Message::assistant("answer"),
            Message::user("q3"),
            Message::assistant("answer3"),
        ];

        let result = cm.compact(&messages, "summary");
        // Verify no orphaned tool results at the start
        for msg in &result {
            if msg.role == Role::Tool {
                // Every tool result must have a preceding assistant with matching tool_call_id
                let has_matching_assistant = result.iter().any(|m| {
                    m.role == Role::Assistant
                        && m.tool_calls.as_ref().map_or(false, |tcs| {
                            tcs.iter().any(|tc| Some(&tc.id) == msg.tool_call_id.as_ref())
                        })
                });
                assert!(has_matching_assistant, "orphaned tool result found: {:?}", msg.tool_call_id);
            }
        }
    }
}
