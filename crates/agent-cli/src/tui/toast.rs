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
    pub(crate) toasts: Vec<Toast>,
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

    pub fn len(&self) -> usize {
        self.toasts.len()
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn test_toast_expiry() {
        let t = Toast::info("test");
        assert!(!t.is_expired());
    }

    #[test]
    fn test_toast_border_color() {
        assert_eq!(Toast::info("a").border_color(), theme::PRIMARY);
        assert_eq!(Toast::success("a").border_color(), theme::SUCCESS);
        assert_eq!(Toast::warning("a").border_color(), theme::WARNING);
        assert_eq!(Toast::error("a").border_color(), theme::ERROR);
    }

    #[test]
    fn test_toast_manager_max() {
        let mut mgr = ToastManager::new();
        for i in 0..10 {
            mgr.push(Toast::info(&format!("msg {}", i)));
        }
        // After pushing 10, only last 5 should remain
        assert_eq!(mgr.toasts.len(), 5);
        // The oldest 5 were dropped, so first remaining should be "msg 5"
        assert_eq!(mgr.toasts[0].message, "msg 5");
    }

    #[test]
    fn test_toast_manager_tick_removes_expired() {
        let mut mgr = ToastManager::new();
        let mut t = Toast::info("gone");
        t.duration = Duration::ZERO;
        t.created_at = Instant::now();
        mgr.push(t);
        mgr.push(Toast::info("stay"));
        mgr.tick();
        assert_eq!(mgr.toasts.len(), 1);
        assert_eq!(mgr.toasts[0].message, "stay");
    }

    #[test]
    fn test_toast_render() {
        let mut mgr = ToastManager::new();
        mgr.push(Toast::info("hello"));
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| {
            mgr.render(f, f.area());
        }).unwrap();
        let buf = terminal.backend().buffer().clone();
        let mut content = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                content.push(buf[(x, y)].symbol().chars().next().unwrap_or(' '));
            }
        }
        assert!(content.contains("Info"), "should render toast label: {content}");
    }
}
