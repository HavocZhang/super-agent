use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;

use crate::tui::markdown::MarkdownRenderer;
use crate::tui::theme;
use crate::tui::tool_block::ToolBlock;

pub struct MessagesArea {
    messages: Vec<ChatMessage>,
    scroll_offset: usize,
    auto_scroll: bool,
    markdown: MarkdownRenderer,
}

#[derive(Debug, Clone)]
pub enum ChatMessage {
    User(String),
    Assistant(String),
    ToolCall(ToolBlock),
    System(String),
    Error(String),
}

impl MessagesArea {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            scroll_offset: 0,
            auto_scroll: true,
            markdown: MarkdownRenderer::new(),
        }
    }

    pub fn push_user(&mut self, text: &str) {
        self.messages.push(ChatMessage::User(text.to_string()));
        if self.auto_scroll {
            self.scroll_offset = 0;
        }
    }

    pub fn push_assistant(&mut self, text: &str) {
        self.messages.push(ChatMessage::Assistant(text.to_string()));
        if self.auto_scroll {
            self.scroll_offset = 0;
        }
    }

    pub fn push_tool(&mut self, block: ToolBlock) {
        self.messages.push(ChatMessage::ToolCall(block));
        if self.auto_scroll {
            self.scroll_offset = 0;
        }
    }

    pub fn push_system(&mut self, text: &str) {
        self.messages.push(ChatMessage::System(text.to_string()));
        if self.auto_scroll {
            self.scroll_offset = 0;
        }
    }

    pub fn push_error(&mut self, text: &str) {
        self.messages.push(ChatMessage::Error(text.to_string()));
        if self.auto_scroll {
            self.scroll_offset = 0;
        }
    }

    pub fn last_is_assistant(&self) -> bool {
        self.messages
            .last()
            .map(|m| matches!(m, ChatMessage::Assistant(_)))
            .unwrap_or(false)
    }

    pub fn append_to_last(&mut self, text: &str) {
        if let Some(last) = self.messages.last_mut() {
            match last {
                ChatMessage::Assistant(ref mut content) => {
                    content.push_str(text);
                }
                _ => {
                    self.messages.push(ChatMessage::Assistant(text.to_string()));
                }
            }
        } else {
            self.messages.push(ChatMessage::Assistant(text.to_string()));
        }
        if self.auto_scroll {
            self.scroll_offset = 0;
        }
    }

    pub fn clear(&mut self) {
        self.messages.clear();
        self.scroll_offset = 0;
        self.auto_scroll = true;
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let all_lines = self.collect_lines(area.width);
        let visible_height = area.height as usize;

        let total_lines = all_lines.len();
        let max_scroll = total_lines.saturating_sub(visible_height);

        let scroll = if self.auto_scroll {
            0
        } else {
            self.scroll_offset.min(max_scroll)
        };

        let start = max_scroll.saturating_sub(scroll);
        let end = (start + visible_height).min(total_lines);
        let visible: Vec<Line<'static>> = all_lines[start..end].to_vec();

        let para = Paragraph::new(Text::from(visible)).wrap(Wrap { trim: false });
        frame.render_widget(para, area);
    }

    fn collect_lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> = Vec::new();
        let content_width = width.saturating_sub(2) as usize;

        for msg in &self.messages {
            match msg {
                ChatMessage::User(text) => {
                    lines.push(Line::from(vec![
                        Span::styled(
                            "┃ ".to_string(),
                            Style::default().fg(theme::PRIMARY).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(text.clone(), Style::default().fg(theme::TEXT)),
                    ]));
                }
                ChatMessage::Assistant(text) => {
                    let rendered = self.markdown.render(text);
                    for mut line in rendered.lines {
                        // Indent assistant lines
                        let mut new_spans = vec![Span::raw("  ")];
                        new_spans.append(&mut line.spans);
                        lines.push(Line::from(new_spans));
                    }
                }
                ChatMessage::ToolCall(block) => {
                    self.collect_tool_lines(&mut lines, block, content_width);
                }
                ChatMessage::System(text) => {
                    let wrapped = wrap_text(text, content_width.saturating_sub(2));
                    for (i, line_text) in wrapped.iter().enumerate() {
                        let prefix = if i == 0 { "  " } else { "  " };
                        lines.push(Line::from(vec![
                            Span::styled(
                                prefix.to_string(),
                                Style::default().fg(theme::TEXT_DIM),
                            ),
                            Span::styled(
                                line_text.clone(),
                                Style::default()
                                    .fg(theme::TEXT_DIM)
                                    .add_modifier(Modifier::ITALIC),
                            ),
                        ]));
                    }
                }
                ChatMessage::Error(text) => {
                    let wrapped = wrap_text(text, content_width.saturating_sub(4));
                    for (i, line_text) in wrapped.iter().enumerate() {
                        let prefix = if i == 0 { "✗ " } else { "  " };
                        lines.push(Line::from(vec![
                            Span::styled(
                                prefix.to_string(),
                                Style::default().fg(theme::ERROR),
                            ),
                            Span::styled(line_text.clone(), Style::default().fg(theme::ERROR)),
                        ]));
                    }
                }
            }
        }

        if lines.is_empty() {
            lines.push(Line::from(Span::styled(
                "  Ready — type a message to begin.",
                Style::default().fg(theme::TEXT_DIM),
            )));
        }

        lines
    }

    fn collect_tool_lines(
        &self,
        lines: &mut Vec<Line<'static>>,
        block: &ToolBlock,
        _content_width: usize,
    ) {
        let display = match block.name.as_str() {
            "file_read" => "Read File",
            "file_write" => "Write File",
            "file_edit" => "Edit File",
            "shell" => "Run Command",
            "grep" => "Search",
            "glob" => "Find Files",
            "ls" => "List Directory",
            _ => block.name.as_str(),
        };
        let color = theme::tool_color(&block.name);

        if block.collapsed {
            let status = match &block.state {
                crate::tui::tool_block::ToolState::Running => " ⏳".to_string(),
                crate::tui::tool_block::ToolState::Success(d) => {
                    format!(" ✓ {}", crate::tui::spinner::fmt_elapsed(*d))
                }
                crate::tui::tool_block::ToolState::Error(_) => " ✗".to_string(),
            };

            let status_color = match &block.state {
                crate::tui::tool_block::ToolState::Success(_) => theme::SUCCESS,
                crate::tui::tool_block::ToolState::Error(_) => theme::ERROR,
                _ => theme::SPINNER,
            };

            let args_preview = if block.arguments.len() > 40 {
                format!("{}…", &block.arguments[..38])
            } else {
                block.arguments.clone()
            };

            lines.push(Line::from(vec![
                Span::styled("  ▸ ", Style::default().fg(color)),
                Span::styled(
                    display.to_string(),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::raw(": "),
                Span::styled(args_preview, Style::default().fg(theme::TEXT_DIM)),
                Span::styled(status, Style::default().fg(status_color)),
            ]));
        } else {
            // Expanded tool block
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  ┌─ {} ", display),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "─".repeat(30),
                    Style::default().fg(color),
                ),
            ]));

            // Arguments
            lines.push(Line::from(vec![
                Span::styled("  │ ", Style::default().fg(color)),
                Span::styled(
                    format!("$ {}", block.arguments),
                    Style::default().fg(theme::TEXT),
                ),
            ]));

            // Output
            if !block.output.is_empty() {
                for line in block.output.lines() {
                    lines.push(Line::from(vec![
                        Span::styled("  │ ", Style::default().fg(color)),
                        Span::styled(line.to_string(), Style::default().fg(theme::TEXT_DIM)),
                    ]));
                }
            }

            // Error
            if let crate::tui::tool_block::ToolState::Error(ref err) = block.state {
                lines.push(Line::from(vec![
                    Span::styled("  │ ", Style::default().fg(color)),
                    Span::styled(err.clone(), Style::default().fg(theme::ERROR)),
                ]));
            }

            // Footer
            let (status_icon, status_color, duration) = match &block.state {
                crate::tui::tool_block::ToolState::Running => ("⏳", theme::SPINNER, String::new()),
                crate::tui::tool_block::ToolState::Success(d) => {
                    ("✓", theme::SUCCESS, format!(" {}", crate::tui::spinner::fmt_elapsed(*d)))
                }
                crate::tui::tool_block::ToolState::Error(_) => ("✗", theme::ERROR, String::new()),
            };

            lines.push(Line::from(vec![
                Span::styled(
                    format!("  └─ {}{} ", status_icon, duration),
                    Style::default().fg(status_color),
                ),
            ]));
        }
    }

    pub fn scroll_up(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(lines);
        self.auto_scroll = false;
    }

    pub fn scroll_down(&mut self, lines: usize) {
        if lines >= self.scroll_offset {
            self.scroll_offset = 0;
            self.auto_scroll = true;
        } else {
            self.scroll_offset -= lines;
        }
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
        self.auto_scroll = true;
    }
}

fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }

    let mut result = Vec::new();
    for line in text.lines() {
        if unicode_width::UnicodeWidthStr::width(line) <= max_width {
            result.push(line.to_string());
        } else {
            let mut current = String::new();
            let mut current_width = 0;
            for ch in line.chars() {
                let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                if current_width + w > max_width {
                    result.push(current);
                    current = String::new();
                    current_width = 0;
                }
                current.push(ch);
                current_width += w;
            }
            if !current.is_empty() {
                result.push(current);
            }
        }
    }
    if result.is_empty() {
        result.push(String::new());
    }
    result
}
