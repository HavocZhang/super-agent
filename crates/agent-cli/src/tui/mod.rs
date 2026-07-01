pub mod app;
pub mod approval;
pub mod component;
pub mod footer;
pub mod header;
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

pub struct TuiApp {
    terminal: ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    app: App,
    engine: Arc<AgentEngine>,
    session_store: Option<SessionStore>,
    current_session_id: String,
    _mcp_manager: McpManager,
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
        execute!(stdout, EnterAlternateScreen, EnableBracketedPaste)?;
        let backend = ratatui::backend::CrosstermBackend::new(stdout);
        let terminal = ratatui::Terminal::new(backend)?;

        Ok(Self {
            terminal,
            app: App::new(model, working_dir),
            engine,
            session_store,
            current_session_id: String::new(),
            _mcp_manager: mcp_manager,
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
            self.terminal.draw(|frame| self.app.render(frame))?;

            if event::poll(std::time::Duration::from_millis(16))? {
                match event::read()? {
                    Event::Key(key) => {
                        if self.handle_key(key).await? {
                            break;
                        }
                    }
                    Event::Paste(data) => {
                        self.app.input.push_str(&data);
                        self.app.input_cursor = self.app.input.len();
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
            DisableBracketedPaste
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
                    self.handle_user_input(&input).await?;
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

    async fn handle_user_input(&mut self, input: &str) -> anyhow::Result<()> {
        if input.starts_with('/') {
            self.handle_command(input).await?;
            return Ok(());
        }

        let expanded = crate::resolve_at_references(input);

        self.app.messages.push_user(input);
        self.app.status = "Thinking...".to_string();
        self.app.spinner.start();
        self.app.header.set_streaming(true);

        let stream = self.engine.run_stream(&expanded).await;
        let mut agent_response = String::new();
        let mut tool_snapshots: Vec<(String, String)> = Vec::new();

        tokio::pin!(stream);
        while let Some(event) = stream.next().await {
            match event {
                Ok(StreamEvent::Token(token)) => {
                    agent_response.push_str(&token);
                    if self.app.messages.last_is_assistant() {
                        self.app.messages.append_to_last(&token);
                    } else {
                        self.app.messages.push_assistant(&token);
                    }
                    self.terminal.draw(|frame| self.app.render(frame))?;
                }
                Ok(StreamEvent::ToolCallStart { ref name, .. }) => {
                    use crate::tui::tool_block::ToolBlock;
                    self.app.messages.push_tool(ToolBlock::new(name, ""));
                    self.terminal.draw(|frame| self.app.render(frame))?;
                }
                Ok(StreamEvent::ToolSnapshot { path, content }) => {
                    tool_snapshots.push((path, content));
                }
                Ok(StreamEvent::Done) => break,
                Ok(StreamEvent::Error(e)) => {
                    self.app.messages.push_error(&e);
                    break;
                }
                Err(e) => {
                    self.app.messages.push_error(&e.to_string());
                    break;
                }
                _ => {}
            }
        }

        self.app.spinner.stop();
        self.app.status = "Ready".to_string();
        self.app.header.set_streaming(false);

        for (path, old_content) in &tool_snapshots {
            if let Ok(new_content) = std::fs::read_to_string(path) {
                if old_content != &new_content {
                    let diff = agent_core::FileDiff::diff(old_content, &new_content, path);
                    self.app
                        .messages
                        .push_system(&format!("Changes in {}:\n{}", path, diff));
                }
            }
        }

        if let Some(ref store) = self.session_store {
            if !self.current_session_id.is_empty() {
                let _ = store.append_message(
                    &self.current_session_id,
                    &SessionMessage {
                        role: "user".to_string(),
                        content: input.to_string(),
                        tool_calls: None,
                        tool_call_id: None,
                        timestamp: Utc::now().to_rfc3339(),
                    },
                );
                if !agent_response.is_empty() {
                    let _ = store.append_message(
                        &self.current_session_id,
                        &SessionMessage {
                            role: "assistant".to_string(),
                            content: agent_response,
                            tool_calls: None,
                            tool_call_id: None,
                            timestamp: Utc::now().to_rfc3339(),
                        },
                    );
                }
            }
        }

        Ok(())
    }

    async fn handle_command(&mut self, input: &str) -> anyhow::Result<()> {
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
                self.app
                    .messages
                    .push_system(&format!("Model: {}", self.app.model));
            }
            _ => {
                self.app.messages.push_system(&format!(
                    "Command '{}' not yet supported in TUI mode",
                    cmd
                ));
            }
        }
        Ok(())
    }
}

impl Drop for TuiApp {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            std::io::stdout(),
            LeaveAlternateScreen,
            DisableBracketedPaste
        );
    }
}
