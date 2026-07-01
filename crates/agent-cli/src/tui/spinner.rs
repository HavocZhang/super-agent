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
