use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::tui::theme;

const BRAILLE_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub struct Footer {
    pub directory: String,
    pub status: String,
    pub model: String,
    pub token_pct: u8,
    pub streaming: bool,
    spinner_tick: u8,
}

impl Footer {
    pub fn new() -> Self {
        Self {
            directory: String::new(),
            status: String::from("Ready"),
            model: String::new(),
            token_pct: 0,
            streaming: false,
            spinner_tick: 0,
        }
    }

    pub fn set_directory(&mut self, dir: &str) {
        self.directory = dir.to_string();
    }

    pub fn set_status(&mut self, status: &str) {
        self.status = status.to_string();
    }

    pub fn set_model(&mut self, model: &str) {
        self.model = model.to_string();
    }

    pub fn set_token_pct(&mut self, pct: u8) {
        self.token_pct = pct.min(100);
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

        let total_width = area.width as usize;
        let mut spans: Vec<Span<'static>> = Vec::new();

        // Left section: directory + spinner + status
        let dir_display = self.truncate_left(&self.directory, 25);
        if !dir_display.is_empty() {
            spans.push(Span::styled(
                dir_display,
                Style::default().fg(theme::TEXT_DIM),
            ));
            spans.push(Span::raw("  "));
        }

        // Spinner
        if self.streaming {
            let frame_idx = (self.spinner_tick as usize) % BRAILLE_FRAMES.len();
            spans.push(Span::styled(
                BRAILLE_FRAMES[frame_idx].to_string(),
                Style::default().fg(theme::SPINNER),
            ));
            spans.push(Span::raw(" "));
        }

        // Status
        let status_color = if self.status.contains("Error") {
            theme::ERROR
        } else if self.streaming {
            theme::WARNING
        } else {
            theme::TEXT_MUTED
        };
        spans.push(Span::styled(
            self.status.clone(),
            Style::default().fg(status_color),
        ));

        // Left width
        let left_width: usize = spans
            .iter()
            .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
            .sum();

        // Right section: model + token% + keybinds
        let mut right_spans: Vec<Span<'static>> = Vec::new();

        // Model
        if !self.model.is_empty() {
            right_spans.push(Span::styled(
                self.model.clone(),
                Style::default().fg(theme::SECONDARY),
            ));
            right_spans.push(Span::raw("  "));
        }

        // Token percentage
        right_spans.push(Span::styled(
            format!("{}%", self.token_pct),
            Style::default().fg(theme::TEXT_DIM),
        ));
        right_spans.push(Span::raw("  "));

        // Keybinds
        right_spans.push(Span::styled(
            "^C",
            Style::default().fg(theme::TEXT_DIM).add_modifier(Modifier::BOLD),
        ));
        right_spans.push(Span::styled(
            " quit ",
            Style::default().fg(theme::TEXT_DIM),
        ));
        right_spans.push(Span::styled(
            "^H",
            Style::default().fg(theme::TEXT_DIM).add_modifier(Modifier::BOLD),
        ));
        right_spans.push(Span::styled(
            " help",
            Style::default().fg(theme::TEXT_DIM),
        ));

        let right_width: usize = right_spans
            .iter()
            .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
            .sum();

        // Fill space between left and right
        let used = left_width + right_width;
        if used < total_width {
            let fill = total_width - used;
            spans.push(Span::raw(" ".repeat(fill)));
        }
        spans.extend(right_spans);

        let para = Paragraph::new(Line::from(spans));
        frame.render_widget(para, area);
    }

    fn truncate_left(&self, s: &str, max_width: usize) -> String {
        let str_width = UnicodeWidthStr::width(s);
        if str_width <= max_width {
            return s.to_string();
        }

        let chars: Vec<char> = s.chars().collect();
        let mut width = 0;
        let mut start = chars.len();
        for (i, ch) in chars.iter().enumerate().rev() {
            let w = unicode_width::UnicodeWidthChar::width(*ch).unwrap_or(0);
            if width + w > max_width - 1 {
                start = i + 1;
                break;
            }
            width += w;
            if i == 0 {
                start = 0;
            }
        }

        let suffix: String = chars[start..].iter().collect();
        format!("…{}", suffix)
    }
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
    fn test_footer_render() {
        let mut f = Footer::new();
        f.set_directory("~/projects/app");
        f.set_status("Ready");
        f.set_model("gpt-4o");
        f.set_token_pct(55);
        let buf = test_render(80, 1, |f_| {
            f.render(f_, f_.area());
        });
        let content = buf_to_string(&buf);
        assert!(content.contains("~/projects/app"), "should show directory: {content}");
        assert!(content.contains("Ready"), "should show status: {content}");
        assert!(content.contains("gpt-4o"), "should show model: {content}");
        assert!(content.contains("55%"), "should show token pct: {content}");
        assert!(content.contains("^C"), "should show keybinds: {content}");
    }

    #[test]
    fn test_truncate_left() {
        let f = Footer::new();
        let result = f.truncate_left("/very/long/path/to/project", 12);
        assert!(result.starts_with('…'), "should start with ellipsis: {result}");
        assert!(result.len() <= 12 || result.chars().count() <= 12);
    }

    #[test]
    fn test_streaming_spinner() {
        let mut f = Footer::new();
        f.set_streaming(true);
        f.tick();
        let buf = test_render(40, 1, |f_| {
            f.render(f_, f_.area());
        });
        let content = buf_to_string(&buf);
        // Braille spinner characters should be present
        assert!(
            content.contains('⠋') || content.contains('⠙') || content.contains('⠹')
                || content.contains('⠸') || content.contains('⠼') || content.contains('⠴')
                || content.contains('⠦') || content.contains('⠧') || content.contains('⠇')
                || content.contains('⠏'),
            "streaming should show braille spinner: {content}"
        );
    }
}
