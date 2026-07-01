use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::tui::theme;

const BAR_WIDTH: usize = 10;
const BAR_EMPTY: char = '░';
const BAR_FILLED: char = '▓';

pub struct Header {
    pub session_title: String,
    pub model: String,
    pub context_pct: u8,
    pub streaming: bool,
    spinner_tick: u8,
}

impl Header {
    pub fn new() -> Self {
        Self {
            session_title: String::from("New Session"),
            model: String::new(),
            context_pct: 0,
            streaming: false,
            spinner_tick: 0,
        }
    }

    pub fn set_model(&mut self, model: &str) {
        self.model = model.to_string();
    }

    pub fn set_session_title(&mut self, title: &str) {
        self.session_title = title.to_string();
    }

    pub fn set_context_pct(&mut self, pct: u8) {
        self.context_pct = pct.min(100);
    }

    pub fn set_streaming(&mut self, streaming: bool) {
        self.streaming = streaming;
    }

    pub fn tick(&mut self) {
        self.spinner_tick = self.spinner_tick.wrapping_add(1);
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        if area.height == 0 {
            return;
        }

        let mut spans: Vec<Span<'static>> = Vec::new();

        // Brand
        spans.push(Span::styled(
            "◆ ",
            Style::default().fg(theme::PRIMARY).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            "agent ",
            Style::default().fg(theme::TEXT).add_modifier(Modifier::BOLD),
        ));

        // Separator
        spans.push(Span::styled("│ ", Style::default().fg(theme::BORDER)));

        // Session title (truncate if needed)
        let title_display = truncate_str(&self.session_title, 30);
        spans.push(Span::styled(
            title_display,
            Style::default().fg(theme::TEXT_MUTED),
        ));

        // Separator
        spans.push(Span::styled("  ", Style::default()));

        // Model name
        if !self.model.is_empty() {
            spans.push(Span::styled(
                self.model.clone(),
                Style::default().fg(theme::SECONDARY),
            ));
        }

        // Calculate left side width and fill remaining space
        let left_width: usize = spans.iter().map(|s| UnicodeWidthStr::width(s.content.as_ref())).sum();

        // Context bar section
        let bar_spans = self.render_context_bar();
        let bar_width: usize = bar_spans.iter().map(|s| UnicodeWidthStr::width(s.content.as_ref())).sum();

        let total_width = area.width as usize;
        if left_width + bar_width < total_width {
            let fill = total_width - left_width - bar_width;
            spans.push(Span::raw(" ".repeat(fill)));
        }
        spans.extend(bar_spans);

        let para = Paragraph::new(Line::from(spans));
        frame.render_widget(para, area);
    }

    fn render_context_bar(&self) -> Vec<Span<'static>> {
        let filled = (self.context_pct as usize * BAR_WIDTH) / 100;
        let empty = BAR_WIDTH.saturating_sub(filled);

        let color = theme::context_bar_color(self.context_pct);

        let bar: String = BAR_FILLED.to_string().repeat(filled)
            + &BAR_EMPTY.to_string().repeat(empty);

        vec![
            Span::styled(bar, Style::default().fg(color)),
            Span::raw(format!(" {}%", self.context_pct)),
        ]
    }
}

fn truncate_str(s: &str, max_width: usize) -> String {
    let mut width = 0;
    let mut result = String::new();
    for ch in s.chars() {
        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + w > max_width {
            result.push('…');
            break;
        }
        result.push(ch);
        width += w;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn test_render<F: FnOnce(&mut ratatui::Frame)>(width: u16, height: u16, render_fn: F) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render_fn(f)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn buf_to_string(buf: &ratatui::buffer::Buffer) -> String {
        let mut s = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                s.push(buf[(x, y)].symbol().chars().next().unwrap_or(' '));
            }
        }
        s
    }

    #[test]
    fn test_header_render() {
        let mut h = Header::new();
        h.set_model("gpt-4o");
        h.set_context_pct(42);
        let buf = test_render(80, 1, |f| {
            h.render(f, f.area());
        });
        let content = buf_to_string(&buf);
        assert!(content.contains("agent"), "should show brand: {content}");
        assert!(content.contains("gpt-4o"), "should show model: {content}");
        assert!(content.contains("42%"), "should show context pct: {content}");
    }

    #[test]
    fn test_context_bar_colors() {
        // Test that different context_pct values render correctly via public API
        for pct in [0u8, 50, 80, 95] {
            let mut h = Header::new();
            h.set_context_pct(pct);
            let buf = test_render(80, 1, |f| {
                h.render(f, f.area());
            });
            let content = buf_to_string(&buf);
            assert!(content.contains(&format!("{}%", pct)), "should show pct={pct}: {content}");
        }
        // Verify color thresholds via theme function
        assert_eq!(theme::context_bar_color(0), theme::PRIMARY);
        assert_eq!(theme::context_bar_color(50), theme::PRIMARY);
        assert_eq!(theme::context_bar_color(80), theme::WARNING);
        assert_eq!(theme::context_bar_color(95), theme::ERROR);
    }

    #[test]
    fn test_truncate_str() {
        assert_eq!(truncate_str("hello", 10), "hello");
        assert_eq!(truncate_str("hello world long", 8), "hello wo…");
        assert_eq!(truncate_str("", 5), "");
    }
}
