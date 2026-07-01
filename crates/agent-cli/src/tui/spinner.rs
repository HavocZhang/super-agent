use std::time::{Duration, Instant};

const BRAILLE_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const FRAME_INTERVAL: Duration = Duration::from_millis(80);

pub struct Spinner {
    tick: usize,
    last_frame: Instant,
    start_time: Instant,
    label: String,
    running: bool,
}

impl Spinner {
    pub fn new(label: &str) -> Self {
        Self {
            tick: 0,
            last_frame: Instant::now(),
            start_time: Instant::now(),
            label: label.to_string(),
            running: false,
        }
    }

    pub fn start(&mut self) {
        self.running = true;
        self.start_time = Instant::now();
        self.last_frame = Instant::now();
        self.tick = 0;
    }

    pub fn stop(&mut self) {
        self.running = false;
    }

    pub fn tick(&mut self) -> &str {
        if !self.running {
            return "";
        }
        let now = Instant::now();
        if now.duration_since(self.last_frame) >= FRAME_INTERVAL {
            self.tick = (self.tick + 1) % BRAILLE_FRAMES.len();
            self.last_frame = now;
        }
        BRAILLE_FRAMES[self.tick]
    }

    pub fn elapsed_str(&self) -> String {
        fmt_elapsed(self.start_time.elapsed())
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn render_line(&self) -> String {
        if !self.running {
            return String::new();
        }
        let frame = BRAILLE_FRAMES[self.tick];
        let elapsed = self.elapsed_str();
        format!("{} {} {}", frame, self.label, elapsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spinner_start_stop() {
        let mut s = Spinner::new("work");
        assert!(!s.is_running());
        assert_eq!(s.tick(), "");
        assert!(s.render_line().is_empty());

        s.start();
        assert!(s.is_running());
        let frame = s.tick();
        assert!(!frame.is_empty());

        s.stop();
        assert!(!s.is_running());
        assert_eq!(s.tick(), "");
    }

    #[test]
    fn test_spinner_tick() {
        let mut s = Spinner::new("test");
        s.start();
        let first = s.tick().to_string();
        // Fast ticks within FRAME_INTERVAL stay on same frame
        let second = s.tick().to_string();
        assert_eq!(first, second);
    }

    #[test]
    fn test_fmt_elapsed() {
        assert_eq!(fmt_elapsed(Duration::from_secs(5)), "5s");
        assert_eq!(fmt_elapsed(Duration::from_secs(63)), "1m 03s");
        assert_eq!(fmt_elapsed(Duration::from_secs(7389)), "2h 03m 09s");
        assert_eq!(fmt_elapsed(Duration::from_secs(0)), "0s");
    }
}

pub fn fmt_elapsed(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {:02}s", secs / 60, secs % 60)
    } else {
        format!("{}h {:02}m {:02}s", secs / 3600, (secs % 3600) / 60, secs % 60)
    }
}
