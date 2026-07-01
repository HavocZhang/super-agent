use std::time::{Duration, Instant};

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::tui::theme;

const MAX_TOAST_WIDTH: u16 = 60;
const DEFAULT_DURATION: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}

pub struct Toast {
    pub message: String,
    pub kind: ToastKind,
    created_at: Instant,
    duration: Duration,
}

impl Toast {
    pub fn info(msg: &str) -> Self {
        Self {
            message: msg.to_string(),
            kind: ToastKind::Info,
            created_at: Instant::now(),
            duration: DEFAULT_DURATION,
        }
    }

    pub fn success(msg: &str) -> Self {
        Self {
            message: msg.to_string(),
            kind: ToastKind::Success,
            created_at: Instant::now(),
            duration: DEFAULT_DURATION,
        }
    }

    pub fn warning(msg: &str) -> Self {
        Self {
            message: msg.to_string(),
            kind: ToastKind::Warning,
            created_at: Instant::now(),
            duration: DEFAULT_DURATION,
        }
    }

    pub fn error(msg: &str) -> Self {
        Self {
            message: msg.to_string(),
            kind: ToastKind::Error,
            created_at: Instant::now(),
            duration: Duration::from_secs(6),
        }
    }

    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed() >= self.duration
    }

    pub fn border_color(&self) -> Color {
        match self.kind {
            ToastKind::Info => theme::PRIMARY,
            ToastKind::Success => theme::SUCCESS,
            ToastKind::Warning => theme::WARNING,
            ToastKind::Error => theme::ERROR,
        }
    }
}

pub struct ToastManager {
    toasts: Vec<Toast>,
}

impl ToastManager {
    pub fn new() -> Self {
        Self { toasts: Vec::new() }
    }

    pub fn push(&mut self, toast: Toast) {
        self.toasts.push(toast);
        if self.toasts.len() > 5 {
            self.toasts.remove(0);
        }
    }

    pub fn tick(&mut self) {
        self.toasts.retain(|t| !t.is_expired());
    }

    pub fn render(&self, frame: &mut ratatui::Frame, area: Rect) {
        if self.toasts.is_empty() {
            return;
        }

        let toast_width = MAX_TOAST_WIDTH.min(area.width.saturating_sub(2));
        let toast_height: u16 = 3;

        for (i, toast) in self.toasts.iter().enumerate() {
            let y = area.y + 1 + (i as u16) * toast_height;
            if y + toast_height > area.y + area.height {
                break;
            }

            let x = area.x + area.width.saturating_sub(toast_width + 1);
            let toast_area = Rect {
                x,
                y,
                width: toast_width,
                height: toast_height,
            };

            let kind_label = match toast.kind {
                ToastKind::Info => "Info",
                ToastKind::Success => "Success",
                ToastKind::Warning => "Warning",
                ToastKind::Error => "Error",
            };

            let border_color = toast.border_color();
            let block = Block::default()
                .title(format!("─ {} ", kind_label))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color));

            let inner = block.inner(toast_area);
            frame.render_widget(block, toast_area);

            let msg = Paragraph::new(Line::from(Span::styled(
                toast.message.clone(),
                Style::default().fg(theme::TEXT),
            )));
            frame.render_widget(msg, inner);
        }
    }
}
