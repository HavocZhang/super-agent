use std::time::Duration;

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::tui::theme;
use crate::tui::spinner::fmt_elapsed;

#[derive(Debug, Clone)]
pub enum ToolState {
    Running,
    Success(Duration),
    Error(String),
}

#[derive(Debug, Clone)]
pub struct ToolBlock {
    pub name: String,
    pub arguments: String,
    pub output: String,
    pub state: ToolState,
    pub collapsed: bool,
}

impl ToolBlock {
    pub fn new(name: &str, args: &str) -> Self {
        Self {
            name: name.to_string(),
            arguments: args.to_string(),
            output: String::new(),
            state: ToolState::Running,
            collapsed: false,
        }
    }

    pub fn finish_ok(&mut self, output: &str, duration: Duration) {
        self.output = output.to_string();
        self.state = ToolState::Success(duration);
    }

    pub fn finish_err(&mut self, error: &str) {
        self.state = ToolState::Error(error.to_string());
    }

    pub fn toggle_collapse(&mut self) {
        self.collapsed = !self.collapsed;
    }

    pub fn render(&self, frame: &mut ratatui::Frame, area: Rect, width: u16) {
        let display = tool_display_name(&self.name);
        let color = theme::tool_color(&self.name);

        if self.collapsed {
            self.render_collapsed(frame, area, display, color);
        } else {
            self.render_expanded(frame, area, width, display, color);
        }
    }

    fn render_collapsed(&self, frame: &mut ratatui::Frame, area: Rect, display: &str, color: ratatui::style::Color) {
        let status_suffix = match &self.state {
            ToolState::Running => " ⏳".to_string(),
            ToolState::Success(d) => format!(" ✓ {}", fmt_elapsed(*d)),
            ToolState::Error(_) => " ✗".to_string(),
        };

        let args_preview = truncate_chars(&self.arguments, 40);

        let line = Line::from(vec![
            Span::styled("▸ ", Style::default().fg(color)),
            Span::styled(display, Style::default().fg(color).add_modifier(Modifier::BOLD)),
            Span::raw(": "),
            Span::styled(args_preview, Style::default().fg(theme::TEXT_DIM)),
            Span::styled(status_suffix, Style::default().fg(match &self.state {
                ToolState::Success(_) => theme::SUCCESS,
                ToolState::Error(_) => theme::ERROR,
                _ => theme::SPINNER,
            })),
        ]);

        let para = Paragraph::new(line);
        frame.render_widget(para, area);
    }

    fn render_expanded(&self, frame: &mut ratatui::Frame, area: Rect, width: u16, display: &str, color: ratatui::style::Color) {
        let (status_icon, status_color, duration_str) = match &self.state {
            ToolState::Running => ("⏳", theme::SPINNER, String::new()),
            ToolState::Success(d) => ("✓", theme::SUCCESS, format!(" {}", fmt_elapsed(*d))),
            ToolState::Error(_) => ("✗", theme::ERROR, String::new()),
        };

        let title = format!(" {} ", display);
        let footer = format!(" {} {}{} ", "─", status_icon, duration_str);

        let border_style = Style::default().fg(color);
        let block = Block::default()
            .title(Span::styled(title, Style::default().fg(color).add_modifier(Modifier::BOLD)))
            .title_bottom(Span::styled(footer, Style::default().fg(status_color)))
            .borders(Borders::ALL)
            .border_style(border_style);

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let content_width = inner.width as usize;
        let mut lines: Vec<Line> = Vec::new();

        // Arguments line
        let args_line = format!("$ {}", self.arguments);
        for wrapped in wrap_text(&args_line, content_width) {
            lines.push(Line::from(Span::styled(wrapped, Style::default().fg(theme::TEXT))));
        }

        // Output
        if !self.output.is_empty() {
            for wrapped in wrap_text(&self.output, content_width) {
                lines.push(Line::from(Span::styled(wrapped, Style::default().fg(theme::TEXT_DIM))));
            }
        }

        // Error
        if let ToolState::Error(ref err) = self.state {
            for wrapped in wrap_text(err, content_width) {
                lines.push(Line::from(Span::styled(wrapped, Style::default().fg(theme::ERROR))));
            }
        }

        let para = Paragraph::new(lines);
        frame.render_widget(para, inner);
    }
}

fn tool_display_name(name: &str) -> &str {
    match name {
        "file_read" => "Read File",
        "file_write" => "Write File",
        "file_edit" => "Edit File",
        "shell" => "Run Command",
        "grep" => "Search",
        "glob" => "Find Files",
        "ls" => "List Directory",
        "git_diff" => "Git Diff",
        "git_status" => "Git Status",
        "git_commit" => "Git Commit",
        "web_search" => "Web Search",
        _ => name,
    }
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    for line in text.split('\n') {
        if unicode_width::UnicodeWidthStr::width(line) <= width {
            lines.push(line.to_string());
        } else {
            let mut current = String::new();
            let mut current_width = 0;
            for ch in line.chars() {
                let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                if current_width + w > width {
                    lines.push(current);
                    current = String::new();
                    current_width = 0;
                }
                current.push(ch);
                current_width += w;
            }
            if !current.is_empty() {
                lines.push(current);
            }
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max_chars.saturating_sub(1)).collect::<String>())
    }
}
