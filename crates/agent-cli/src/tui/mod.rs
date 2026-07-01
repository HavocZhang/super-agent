pub mod app;
pub mod approval;
pub mod component;
pub mod footer;
pub mod header;
pub mod line_buffer;
pub mod markdown;
pub mod messages;
pub mod spinner;
pub mod theme;
pub mod toast;
pub mod tool_block;

use app::App;
use agent_core::{AgentEngine, SessionMessage, SessionStore};
use agent_llm::StreamEvent;
use agent_tools::McpManager;
use chrono::Utc;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::event::{DisableBracketedPaste, EnableBracketedPaste};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use futures_util::StreamExt;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

const MIN_FRAME_INTERVAL: Duration = Duration::from_millis(8);

struct FrameRateLimiter {
    last_draw: Option<Instant>,
}

impl FrameRateLimiter {
    fn new() -> Self {
        Self { last_draw: None }
    }

    fn should_draw(&mut self) -> bool {
        let now = Instant::now();
        if let Some(last) = self.last_draw {
            if now.duration_since(last) < MIN_FRAME_INTERVAL {
                return false;
            }
        }
        self.last_draw = Some(now);
        true
    }
}

/// Events from the background agent task to the main UI loop.
enum AgentEvent {
    /// Streaming text token
    Token(String),
    /// A tool call started (name)
    ToolCallStart(String),
    /// Tool call arguments resolved (name, arguments_json)
    ToolCallEnd(String, String),
    /// Tool execution completed (name, output, duration_ms)
    ToolResult(String, String, u64),
    /// File snapshot before edit (path, content)
    ToolSnapshot(String, String),
    /// Agent turn finished normally
    Done,
    /// Agent error
    Error(String),
}

pub struct TuiApp {
    terminal: ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    app: App,
    engine: Arc<AgentEngine>,
    session_store: Option<SessionStore>,
    current_session_id: String,
    _mcp_manager: McpManager,
    limiter: FrameRateLimiter,
    needs_redraw: bool,
    /// Channel for receiving events from the background agent task.
    agent_rx: Option<mpsc::Receiver<AgentEvent>>,
    /// Track which tool is currently executing for timing.
    tool_start_time: Option<Instant>,
}

impl TuiApp {
    pub fn new(
        engine: Arc<AgentEngine>,
        session_store: Option<SessionStore>,
        mcp_manager: McpManager,
        model: &str,
        working_dir: &str,
    ) -> anyhow::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableBracketedPaste, crossterm::event::EnableMouseCapture)?;
        let backend = ratatui::backend::CrosstermBackend::new(stdout);
        let terminal = ratatui::Terminal::new(backend)?;

        Ok(Self {
            terminal,
            app: App::new(model, working_dir),
            engine,
            session_store,
            current_session_id: String::new(),
            _mcp_manager: mcp_manager,
            limiter: FrameRateLimiter::new(),
            needs_redraw: true,
            agent_rx: None,
            tool_start_time: None,
        })
    }

    pub async fn run(&mut self) -> anyhow::Result<()> {
        // Init working dir in footer
        if let Ok(cwd) = std::env::current_dir() {
            let s = cwd.to_string_lossy().to_string();
            if let Some(home) = dirs::home_dir() {
                let h = home.to_string_lossy().to_string();
                if s.starts_with(&h) {
                    self.app.footer.set_directory(&s.replacen(&h, "~", 1));
                } else {
                    self.app.footer.set_directory(&s);
                }
            } else {
                self.app.footer.set_directory(&s);
            }
        }
        self.app.footer.set_model(&self.app.model);

        loop {
            // Drain agent events (non-blocking)
            if let Some(ref mut rx) = self.agent_rx {
                let mut agent_done = false;
                while let Ok(evt) = rx.try_recv() {
                    match evt {
                        AgentEvent::Token(token) => {
                            if self.app.messages.last_is_assistant() {
                                self.app.messages.append_to_last(&token);
                            } else {
                                self.app.messages.push_assistant(&token);
                            }
                            self.needs_redraw = true;
                        }
                        AgentEvent::ToolCallStart(name) => {
                            use crate::tui::tool_block::ToolBlock;
                            self.app.messages.push_tool(ToolBlock::new(&name, ""));
                            self.needs_redraw = true;
                        }
                        AgentEvent::ToolCallEnd(_name, args) => {
                            self.app.messages.update_last_tool_args(&args);
                            self.tool_start_time = Some(Instant::now());
                            self.needs_redraw = true;
                        }
                        AgentEvent::ToolResult(_name, output, dur_ms) => {
                            let dur = Duration::from_millis(dur_ms);
                            self.app.messages.finish_last_tool(&output, dur);
                            self.needs_redraw = true;
                        }
                        AgentEvent::ToolSnapshot(path, content) => {
                            // Store for diff later — handled in finish
                            self.app.pending_snapshots.push((path, content));
                        }
                        AgentEvent::Done => {
                            agent_done = true;
                        }
                        AgentEvent::Error(e) => {
                            self.app.messages.push_error(&e);
                            agent_done = true;
                        }
                    }
                }
                if agent_done {
                    self.finish_turn().await;
                    self.agent_rx = None;
                }
            }

            // Draw
            if self.needs_redraw && self.limiter.should_draw() {
                self.terminal.draw(|frame| self.app.render(frame))?;
                self.needs_redraw = false;
            }

            // Poll events
            if event::poll(Duration::from_millis(8))? {
                match event::read()? {
                    Event::Key(key) => {
                        self.needs_redraw = true;
                        if self.handle_key(key).await? {
                            break;
                        }
                    }
                    Event::Paste(data) => {
                        self.app.input.push_str(&data);
                        self.app.input_cursor = self.app.input.len();
                        self.needs_redraw = true;
                    }
                    Event::Mouse(mouse) => {
                        use crossterm::event::MouseEventKind;
                        match mouse.kind {
                            MouseEventKind::ScrollUp => {
                                self.app.messages.scroll_up(3);
                                self.needs_redraw = true;
                            }
                            MouseEventKind::ScrollDown => {
                                self.app.messages.scroll_down(3);
                                self.needs_redraw = true;
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }

            if !self.app.running {
                break;
            }

            self.app.tick();
        }

        disable_raw_mode()?;
        execute!(
            std::io::stdout(),
            LeaveAlternateScreen,
            DisableBracketedPaste,
            crossterm::event::DisableMouseCapture
        )?;
        Ok(())
    }

    async fn handle_key(&mut self, key: KeyEvent) -> anyhow::Result<bool> {
        // Approval mode
        if self.app.approval.is_visible() {
            match key.code {
                KeyCode::Left => self.app.approval.select_prev(),
                KeyCode::Right => self.app.approval.select_next(),
                KeyCode::Char(' ') => self.app.approval.toggle_selected(),
                KeyCode::Enter => {
                    let _decisions = self.app.approval.confirm();
                    self.app.approval.hide();
                }
                KeyCode::Esc => self.app.approval.reject_all(),
                _ => {}
            }
            return Ok(false);
        }

        // If agent is running, only allow Ctrl+C and Esc
        if self.agent_rx.is_some() {
            match key.code {
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    // TODO: abort the running task
                    self.app.spinner.stop();
                    self.app.status = "Interrupted".to_string();
                    self.app.header.set_streaming(false);
                    self.agent_rx = None;
                    self.needs_redraw = true;
                }
                KeyCode::Esc => {
                    self.app.spinner.stop();
                    self.app.status = "Ready".to_string();
                    self.app.header.set_streaming(false);
                    self.needs_redraw = true;
                }
                _ => {}
            }
            return Ok(false);
        }

        // Normal mode
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok(true);
            }
            KeyCode::Esc => {
                self.app.spinner.stop();
                self.app.status = "Ready".to_string();
                self.app.header.set_streaming(false);
            }
            KeyCode::Enter => {
                if !self.app.input.is_empty() {
                    let input = self.app.input.clone();
                    self.app.input.clear();
                    self.app.input_cursor = 0;
                    self.submit_input(&input).await;
                }
            }
            KeyCode::Char(ch) => {
                self.app.input.insert(self.app.input_cursor, ch);
                self.app.input_cursor += ch.len_utf8();
            }
            KeyCode::Backspace => {
                if self.app.input_cursor > 0 {
                    let prev = self.app.input[..self.app.input_cursor]
                        .chars()
                        .last()
                        .map(|c| c.len_utf8())
                        .unwrap_or(0);
                    self.app.input_cursor -= prev;
                    self.app.input.remove(self.app.input_cursor);
                }
            }
            KeyCode::Delete => {
                if self.app.input_cursor < self.app.input.len() {
                    self.app.input.remove(self.app.input_cursor);
                }
            }
            KeyCode::Left => {
                if self.app.input_cursor > 0 {
                    let prev = self.app.input[..self.app.input_cursor]
                        .chars()
                        .last()
                        .map(|c| c.len_utf8())
                        .unwrap_or(0);
                    self.app.input_cursor -= prev;
                }
            }
            KeyCode::Right => {
                if self.app.input_cursor < self.app.input.len() {
                    let next = self.app.input[self.app.input_cursor..]
                        .chars()
                        .next()
                        .map(|c| c.len_utf8())
                        .unwrap_or(0);
                    self.app.input_cursor += next;
                }
            }
            KeyCode::PageUp => {
                self.app.messages.scroll_up(10);
            }
            KeyCode::PageDown => {
                self.app.messages.scroll_down(10);
            }
            KeyCode::Home => self.app.input_cursor = 0,
            KeyCode::End => self.app.input_cursor = self.app.input.len(),
            _ => {}
        }
        Ok(false)
    }

    /// Submit user input — spawns a background task for agent execution.
    async fn submit_input(&mut self, input: &str) {
        if input.starts_with('/') {
            self.handle_command(input).await;
            return;
        }

        let expanded = crate::resolve_at_references(input);
        self.app.messages.push_user(&expanded);
        self.app.status = "Thinking...".to_string();
        self.app.spinner.start();
        self.app.header.set_streaming(true);
        self.needs_redraw = true;

        // Spawn background agent task
        let (tx, rx) = mpsc::channel::<AgentEvent>(256);
        self.agent_rx = Some(rx);

        let engine = self.engine.clone();
        let user_input = input.to_string();

        tokio::spawn(async move {
            let stream = engine.run_stream(&expanded).await;
            tokio::pin!(stream);

            while let Some(event) = stream.next().await {
                match event {
                    Ok(StreamEvent::Token(token)) => {
                        if tx.send(AgentEvent::Token(token)).await.is_err() { return; }
                    }
                    Ok(StreamEvent::ToolCallStart { name, .. }) => {
                        if tx.send(AgentEvent::ToolCallStart(name)).await.is_err() { return; }
                    }
                    Ok(StreamEvent::ToolCallEnd { name, arguments, .. }) => {
                        let args_str = if arguments.is_string() {
                            arguments.as_str().unwrap_or("").to_string()
                        } else {
                            serde_json::to_string_pretty(&arguments).unwrap_or_default()
                        };
                        if tx.send(AgentEvent::ToolCallEnd(name, args_str)).await.is_err() { return; }
                    }
                    Ok(StreamEvent::ToolResult { name, output, .. }) => {
                        if tx.send(AgentEvent::ToolResult(name, output, 0)).await.is_err() { return; }
                    }
                    Ok(StreamEvent::ToolSnapshot { path, content }) => {
                        let _ = tx.send(AgentEvent::ToolSnapshot(path, content)).await;
                    }
                    Ok(StreamEvent::Done) => {
                        let _ = tx.send(AgentEvent::Done).await;
                        return;
                    }
                    Ok(StreamEvent::Error(e)) => {
                        let _ = tx.send(AgentEvent::Error(e)).await;
                        return;
                    }
                    Err(e) => {
                        let _ = tx.send(AgentEvent::Error(e.to_string())).await;
                        return;
                    }
                    _ => {} // ToolCallDelta and other events ignored
                }
            }
            // Stream ended without Done
            let _ = tx.send(AgentEvent::Done).await;
        });

        // Store user input for session save later
        self.app.pending_user_input = Some(user_input);
    }

    /// Called when the agent turn finishes — save session, show diffs.
    async fn finish_turn(&mut self) {
        self.app.spinner.stop();
        self.app.status = "Ready".to_string();
        self.app.header.set_streaming(false);

        // Show file diffs
        let snapshots = std::mem::take(&mut self.app.pending_snapshots);
        for (path, old_content) in &snapshots {
            if let Ok(new_content) = std::fs::read_to_string(path) {
                if old_content != &new_content {
                    let diff = agent_core::FileDiff::diff(old_content, &new_content, path);
                    self.app.messages.push_system(&format!("Changes in {}:\n{}", path, diff));
                }
            }
        }

        // Save to session
        if let Some(ref store) = self.session_store {
            if !self.current_session_id.is_empty() {
                if let Some(ref input) = self.app.pending_user_input {
                    let _ = store.append_message(&self.current_session_id, &SessionMessage {
                        role: "user".to_string(),
                        content: input.clone(),
                        tool_calls: None,
                        tool_call_id: None,
                        timestamp: Utc::now().to_rfc3339(),
                    });
                }
            }
        }
        self.app.pending_user_input = None;
        self.needs_redraw = true;
    }

    async fn handle_command(&mut self, input: &str) {
        let cmd = input.split_whitespace().next().unwrap_or(input);
        match cmd {
            "/quit" | "/exit" | "/q" => {
                self.app.running = false;
            }
            "/help" | "/h" | "/?" => {
                self.app.messages.push_system(
                    "Commands: /help /model /skills /clear /quit /sessions /new /mcp",
                );
            }
            "/clear" => {
                self.app.messages.clear();
            }
            "/model" => {
                self.app.messages.push_system(&format!("Model: {}", self.app.model));
            }
            _ => {
                self.app.messages.push_system(&format!(
                    "Command '{}' not yet supported in TUI mode",
                    cmd
                ));
            }
        }
        self.needs_redraw = true;
    }
}

impl Drop for TuiApp {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            std::io::stdout(),
            LeaveAlternateScreen,
            DisableBracketedPaste,
            crossterm::event::DisableMouseCapture
        );
    }
}
