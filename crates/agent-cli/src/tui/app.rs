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
        self.spinner.tick();
        self.toasts.tick();
        self.footer.status = self.status.clone();
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

        // Calculate visible portion of input
        let input_display = format!("> {}", self.input);
        let visible_width = inner.width as usize;

        let display_str = if input_display.len() > visible_width {
            let cursor_abs = 2 + self.input_cursor; // "> " prefix
            let start = if cursor_abs > visible_width {
                cursor_abs - visible_width
            } else {
                0
            };
            let end = (start + visible_width).min(input_display.len());
            &input_display[start..end]
        } else {
            &input_display
        };

        let para = Paragraph::new(Line::from(Span::styled(
            display_str.to_string(),
            Style::default().fg(theme::TEXT),
        )));
        frame.render_widget(para, inner);
    }
}
