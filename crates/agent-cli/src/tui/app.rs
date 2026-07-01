use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::tui::approval::ApprovalBar;
use crate::tui::footer::Footer;
use crate::tui::header::Header;
use crate::tui::messages::MessagesArea;
use crate::tui::spinner::Spinner;
use crate::tui::theme;
use crate::tui::toast::ToastManager;

pub struct App {
    pub header: Header,
    pub footer: Footer,
    pub messages: MessagesArea,
    pub approval: ApprovalBar,
    pub spinner: Spinner,
    pub toasts: ToastManager,
    pub input: String,
    pub input_cursor: usize,
    pub running: bool,
    pub model: String,
    pub status: String,
}

impl App {
    pub fn new(model: &str, _working_dir: &str) -> Self {
        let mut header = Header::new();
        header.set_model(model);

        Self {
            header,
            footer: Footer::new(),
            messages: MessagesArea::new(),
            approval: ApprovalBar::new(),
            spinner: Spinner::new("Thinking"),
            toasts: ToastManager::new(),
            input: String::new(),
            input_cursor: 0,
            running: true,
            model: model.to_string(),
            status: "Ready".to_string(),
        }
    }

    pub fn tick(&mut self) {
        self.header.tick();
        self.toasts.tick();
        // Update footer spinner + status
        self.footer.set_streaming(self.spinner.is_running());
        if self.spinner.is_running() {
            let frame = self.spinner.tick();
            if !frame.is_empty() {
                self.footer.set_status(&format!("{} {}", frame, self.status));
            }
        } else {
            self.footer.set_status(&self.status);
        }
    }

    pub fn render(&self, frame: &mut ratatui::Frame) {
        let area = frame.area();

        let approval_h = if self.approval.is_visible() { 4 } else { 0 };
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),              // Header
                Constraint::Min(1),                 // Messages
                Constraint::Length(approval_h),      // Approval (conditional)
                Constraint::Length(3),              // Input box
                Constraint::Length(1),              // Footer
            ])
            .split(area);

        self.header.render(frame, chunks[0]);
        self.messages.render(frame, chunks[1]);
        if self.approval.is_visible() {
            self.approval.render(frame, chunks[2]);
        }
        self.render_input(frame, chunks[3]);
        self.footer.render(frame, chunks[4]);

        self.toasts.render(frame, area);
    }

    fn render_input(&self, frame: &mut ratatui::Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::BORDER))
            .title(" Input ");

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.width == 0 || inner.height == 0 {
            return;
        }

        let visible_width = inner.width as usize;
        let prefix = "> ";
        let cursor_char = "▏";

        // input_cursor is a BYTE offset. Convert to char index for chars().take/skip.
        let char_idx = self.input[..self.input_cursor].chars().count();
        let before: String = self.input.chars().take(char_idx).collect();
        let after: String = self.input.chars().skip(char_idx).collect();

        // Build spans: prefix + before_cursor + cursor + after_cursor
        let mut spans: Vec<Span<'static>> = Vec::new();
        let text_style = Style::default().fg(theme::TEXT);
        let cursor_style = Style::default().fg(theme::PRIMARY);

        spans.push(Span::styled(prefix.to_string(), text_style));
        for ch in before.chars() {
            spans.push(Span::styled(ch.to_string(), text_style));
        }
        spans.push(Span::styled(cursor_char.to_string(), cursor_style));
        for ch in after.chars() {
            spans.push(Span::styled(ch.to_string(), text_style));
        }

        // Calculate total display width and truncate if needed
        let mut total_w = 0usize;
        let mut truncate_at = spans.len();
        for (i, span) in spans.iter().enumerate() {
            let w = unicode_width::UnicodeWidthStr::width(span.content.as_ref());
            if total_w + w > visible_width {
                truncate_at = i;
                break;
            }
            total_w += w;
        }
        spans.truncate(truncate_at);

        let para = Paragraph::new(Line::from(spans));
        frame.render_widget(para, inner);
    }
}
