use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::Frame;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ComponentId(u64);

impl ComponentId {
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EventPropagation {
    Stop,
    Continue,
}

pub trait Component {
    fn id(&self) -> ComponentId;
    fn render(&self, frame: &mut Frame, area: Rect);
    fn handle_key(&mut self, _key: KeyEvent) -> EventPropagation {
        EventPropagation::Continue
    }
    fn update(&mut self, _delta: Duration) {}
    fn is_focusable(&self) -> bool {
        false
    }
    fn focused(&mut self) {}
    fn blurred(&mut self) {}
}
