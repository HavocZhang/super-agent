use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::tui::theme;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ApprovalStatus {
    Approved,
    Rejected,
}

#[derive(Debug, Clone)]
pub struct ApprovalAction {
    pub name: String,
    pub status: ApprovalStatus,
}

pub struct ApprovalBar {
    actions: Vec<ApprovalAction>,
    selected: usize,
    visible: bool,
}

impl ApprovalBar {
    pub fn new() -> Self {
        Self {
            actions: Vec::new(),
            selected: 0,
            visible: false,
        }
    }

    pub fn show(&mut self, tool_names: Vec<String>) {
        self.actions = tool_names
            .into_iter()
            .map(|name| ApprovalAction {
                name,
                status: ApprovalStatus::Approved,
            })
            .collect();
        self.selected = 0;
        self.visible = true;
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn toggle_selected(&mut self) {
        if let Some(action) = self.actions.get_mut(self.selected) {
            action.status = match action.status {
                ApprovalStatus::Approved => ApprovalStatus::Rejected,
                ApprovalStatus::Rejected => ApprovalStatus::Approved,
            };
        }
    }

    pub fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn select_next(&mut self) {
        if self.selected + 1 < self.actions.len() {
            self.selected += 1;
        }
    }

    pub fn confirm(&self) -> Vec<bool> {
        self.actions
            .iter()
            .map(|a| a.status == ApprovalStatus::Approved)
            .collect()
    }

    pub fn reject_all(&mut self) {
        for action in &mut self.actions {
            action.status = ApprovalStatus::Rejected;
        }
        self.visible = false;
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(" Approval Required ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::WARNING));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.height < 2 || self.actions.is_empty() {
            return;
        }

        // Build tool status line
        let mut spans: Vec<Span> = Vec::new();
        for (i, action) in self.actions.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw("   "));
            }

            let is_selected = i == self.selected;
            let (icon, color) = match action.status {
                ApprovalStatus::Approved => ("✓", theme::SUCCESS),
                ApprovalStatus::Rejected => ("✗", theme::ERROR),
            };

            let mut style = Style::default().fg(color);
            if is_selected {
                style = style.add_modifier(Modifier::UNDERLINED | Modifier::BOLD);
            }

            spans.push(Span::styled(format!("{} {}", icon, action.name), style));
        }

        let status_line = Paragraph::new(Line::from(spans));
        let status_area = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        };
        frame.render_widget(status_line, status_area);

        // Help line
        if inner.height >= 2 {
            let help = Paragraph::new(Line::from(vec![
                Span::styled(
                    "space",
                    Style::default()
                        .fg(theme::TEXT_MUTED)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" · ", Style::default().fg(theme::TEXT_DIM)),
                Span::styled(
                    "← →",
                    Style::default()
                        .fg(theme::TEXT_MUTED)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" · ", Style::default().fg(theme::TEXT_DIM)),
                Span::styled(
                    "enter",
                    Style::default()
                        .fg(theme::TEXT_MUTED)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" confirm  ", Style::default().fg(theme::TEXT_DIM)),
                Span::styled(
                    "esc",
                    Style::default()
                        .fg(theme::TEXT_MUTED)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" reject all", Style::default().fg(theme::TEXT_DIM)),
            ]));
            let help_area = Rect {
                x: inner.x,
                y: inner.y + 1,
                width: inner.width,
                height: 1,
            };
            frame.render_widget(help, help_area);
        }
    }
}
