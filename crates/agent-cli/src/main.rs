mod tui;

use agent_core::{AgentConfig, AgentEngine, FileDiff, SessionMessage, SessionStore};
use agent_llm::{OpenAiProvider, StreamEvent};
use agent_tools::default_tools;
use anyhow::Result;
use chrono::Utc;
use futures_util::StreamExt;
use rustyline::completion::{Completer, Pair};
use rustyline::config::Configurer;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::history::DefaultHistory;
use rustyline::validate::Validator;
use rustyline::{Editor, Helper};
use std::borrow::Cow;
use std::io::{self, Write};
use std::path::Path;
use std::sync::Arc;

// ── ANSI helpers ────────────────────────────────────────────

fn bold(s: &str) -> String { format!("\x1b[1m{}\x1b[0m", s) }
fn cyan(s: &str) -> String { format!("\x1b[1;36m{}\x1b[0m", s) }
fn green(s: &str) -> String { format!("\x1b[1;32m{}\x1b[0m", s) }
fn yellow(s: &str) -> String { format!("\x1b[1;33m{}\x1b[0m", s) }
fn red(s: &str) -> String { format!("\x1b[1;31m{}\x1b[0m", s) }
fn dim(s: &str) -> String { format!("\x1b[2m{}\x1b[0m", s) }
fn italic(s: &str) -> String { format!("\x1b[3m{}\x1b[0m", s) }

// ── 配置结构体 ───────────────────────────────────────────────
// 从 TOML 文件或环境变量中读取应用配置

/// 应用顶层配置
/// 从 ~/.agent/config.toml 文件中反序列化
#[derive(serde::Deserialize)]
struct AppConfig {
    /// API密钥 (必填)
    api_key: String,
    /// 自定义API基础URL (可选，默认为 OpenAI)
    base_url: Option<String>,
    /// Agent运行时配置 (有默认值)
    #[serde(default)]
    agent: AgentConfig,
}

/// 加载配置：优先读取 ~/.agent/config.toml，否则从环境变量 OPENAI_API_KEY 获取
fn load_config() -> Result<AppConfig> {
    let config_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".agent")
        .join("config.toml");

    if config_path.exists() {
        let content = std::fs::read_to_string(&config_path)?;
        let config: AppConfig = toml::from_str(&content)?;
        return Ok(config);
    }

    let api_key = std::env::var("OPENAI_API_KEY")
        .or_else(|_| std::env::var("ANTHROPIC_API_KEY"))
        .unwrap_or_else(|_| {
            eprintln!("请设置 OPENAI_API_KEY 环境变量或创建 ~/.agent/config.toml");
            std::process::exit(1);
        });

    Ok(AppConfig {
        api_key,
        base_url: None,
        agent: AgentConfig::default(),
    })
}

// ── Completion Helper ───────────────────────────────────────

struct AgentHelper {
    commands: Vec<String>,
    skills: Vec<String>,
}

impl AgentHelper {
    fn new() -> Self {
        let commands: Vec<String> = vec![
            "/help", "/model", "/skills", "/skill", "/clear", "/memory", "/quit",
            "/sessions", "/new", "/session", "/mcp", "/plan", "/brainstorm",
        ].into_iter().map(String::from).collect();

        Self { commands, skills: list_skill_names() }
    }

    fn list_dir(prefix: &str) -> Vec<String> {
        let (dir, partial) = match prefix.rfind('/') {
            Some(pos) => (prefix[..=pos].to_string(), prefix[pos + 1..].to_string()),
            None => (".".to_string(), prefix.to_string()),
        };
        let mut results = vec![];
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with(&partial) {
                    let full = if dir == "." { name.clone() } else { format!("{}/{}", dir.trim_end_matches('/'), name) };
                    if entry.file_type().unwrap().is_dir() {
                        results.push(format!("{}/", full));
                    } else {
                        results.push(full);
                    }
                }
            }
        }
        results
    }
}

impl Completer for AgentHelper {
    type Candidate = Pair;
    fn complete(&self, line: &str, pos: usize, _ctx: &rustyline::Context<'_>) -> rustyline::Result<(usize, Vec<Pair>)> {
        let start = line[..pos].rfind(' ').map(|i| i + 1).unwrap_or(0);
        let word = &line[start..pos];
        let mut candidates = vec![];

        if word.starts_with('/') {
            if word.starts_with("/skill ") {
                let prefix = &word[7..];
                if prefix.starts_with("use ") {
                    let skill_prefix = &prefix[4..];
                    for skill in &self.skills {
                        if skill.starts_with(skill_prefix) {
                            candidates.push(Pair { display: format!("/skill use {}", skill), replacement: format!("/skill use {} ", skill) });
                        }
                    }
                } else {
                    for skill in &self.skills {
                        if skill.starts_with(prefix) {
                            candidates.push(Pair { display: format!("/skill {}", skill), replacement: format!("/skill {}", skill) });
                        }
                    }
                    candidates.push(Pair { display: "/skill use".to_string(), replacement: "/skill use ".to_string() });
                }
            } else {
                for cmd in &self.commands {
                    if cmd.starts_with(word) {
                        candidates.push(Pair { display: cmd.clone(), replacement: cmd.clone() });
                    }
                }
            }
        } else if word.starts_with('@') {
            let path_prefix = &word[1..];
            for c in Self::list_dir(path_prefix) {
                candidates.push(Pair { display: format!("@{}", c), replacement: format!("@{}", c) });
            }
        }
        Ok((start, candidates))
    }
}

impl Hinter for AgentHelper {
    type Hint = String;
    fn hint(&self, line: &str, pos: usize, _ctx: &rustyline::Context<'_>) -> Option<String> {
        let start = line[..pos].rfind(' ').map(|i| i + 1).unwrap_or(0);
        let word = &line[start..pos];
        if word.is_empty() { return None; }
        if word.starts_with('/') && !word.contains(' ') {
            for cmd in &self.commands {
                if cmd.starts_with(word) && cmd != word {
                    return Some(cmd[word.len()..].to_string());
                }
            }
        }
        None
    }
}

impl Highlighter for AgentHelper {
    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        Cow::Owned(dim(hint))
    }
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        if line.starts_with('/') {
            Cow::Owned(yellow(line))
        } else if line.contains('@') {
            Cow::Owned(cyan(line))
        } else {
            Cow::Borrowed(line)
        }
    }
}

impl Validator for AgentHelper {}
impl Helper for AgentHelper {}

// ── Skill helpers ───────────────────────────────────────────

fn list_skill_names() -> Vec<String> {
    let mut skills = vec![];
    let dirs = [
        dirs::home_dir().map(|h| h.join(".codex/skills")),
        dirs::home_dir().map(|h| h.join(".agents/skills")),
        Some(agent_core::SkillEvolution::default_path()),
    ];
    for dir_opt in &dirs {
        if let Some(dir) = dir_opt {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if !name.starts_with('.') { skills.push(name); }
                }
            }
        }
    }
    skills.sort();
    skills.dedup();
    skills
}

fn read_skill(name: &str) -> Option<String> {
    let dirs = [
        dirs::home_dir().map(|h| h.join(format!(".codex/skills/{}/SKILL.md", name))),
        dirs::home_dir().map(|h| h.join(format!(".agents/skills/{}/SKILL.md", name))),
        Some(agent_core::SkillEvolution::default_path().join(format!("{}/SKILL.md", name))),
    ];
    for dir_opt in &dirs {
        if let Some(path) = dir_opt {
            if path.exists() {
                if let Ok(content) = std::fs::read_to_string(path) {
                    return Some(content);
                }
            }
        }
    }
    None
}

// ── @ reference expansion ───────────────────────────────────

fn resolve_at_references(input: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '@' {
            let path_start = i + 1;
            let mut path_end = path_start;
            while path_end < chars.len() && !chars[path_end].is_whitespace() {
                path_end += 1;
            }
            if path_end > path_start {
                let path_str: String = chars[path_start..path_end].iter().collect();
                let path = Path::new(&path_str);
                if path.is_file() {
                    match std::fs::read_to_string(path) {
                        Ok(content) => {
                            let fname = path.file_name().unwrap_or_default().to_string_lossy();
                            result.push_str(&format!("[File: {}]\n{}\n[End File]\n", fname, content));
                            i = path_end;
                            continue;
                        }
                        Err(e) => {
                            result.push_str(&format!("[Error reading {}: {}]", path_str, e));
                            i = path_end;
                            continue;
                        }
                    }
                } else if path.is_dir() {
                    let mut listing = String::new();
                    if let Ok(entries) = std::fs::read_dir(path) {
                        for entry in entries.flatten() {
                            let name = entry.file_name().to_string_lossy().to_string();
                            if name.starts_with('.') { continue; }
                            let ft = entry.file_type().unwrap();
                            if ft.is_dir() { listing.push_str(&format!("{}/\n", name)); }
                            else { listing.push_str(&format!("{}\n", name)); }
                        }
                    }
                    result.push_str(&format!("[Directory: {}]\n{}[End Directory]\n", path_str, listing));
                    i = path_end;
                    continue;
                }
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

// ── Print helpers ───────────────────────────────────────────

fn banner() {
    println!();
    println!("  {}", cyan("╭──────────────────────────────────────────────╮"));
    println!("  {}           {}                  {}", cyan("│"), bold("🤖 Coding Agent"), cyan("│"));
    println!("  {}                                              {}", cyan("│"), cyan("│"));
    println!("  {}  {}                                       {}", cyan("│"), dim("Commands: /help /model /skills /clear /quit"), cyan("│"));
    println!("  {}  {}                                 {}", cyan("│"), dim("@file / @dir to inject content"), cyan("│"));
    println!("  {}  {}                                      {}", cyan("│"), dim("Ctrl+C to exit"), cyan("│"));
    println!("  {}", cyan("╰──────────────────────────────────────────────╯"));
    println!();
}

fn print_user(text: &str) {
    println!();
    println!("  {} {}", cyan("You"), dim("»"));
    for line in text.lines() {
        println!("    {}", line);
    }
}

fn print_agent_header() {
    print!("  {} ", green("Agent"));
    io::stdout().flush().ok();
}

fn print_agent_token(token: &str) {
    print!("{}", token);
    io::stdout().flush().ok();
}

fn print_agent_newline() {
    println!();
}

fn print_tool_start(name: &str) {
    println!();
    println!("  {} {}", yellow("⚙"), yellow(name));
}

fn print_error(err: &str) {
    println!();
    println!("  {} {}", red("✗"), red(err));
}

fn print_thinking() {
    println!("  {} {}", yellow("🤔"), italic("Thinking..."));
}

fn print_help() {
    println!();
    println!("  {}", bold("Commands:"));
    println!("    {}  Show this help", yellow("/help"));
    println!("    {}  Show current model", yellow("/model"));
    println!("    {}  List installed skills", yellow("/skills"));
    println!("    {}  View skill content", yellow("/skill <name>"));
    println!("    {}  Use skill for a task", yellow("/skill use <name> <task>"));
    println!("    {}  Create a new skill", yellow("/skill create <name>"));
    println!("    {}  Trigger skill evolution", yellow("/skill evolve"));
    println!("    {}  List MCP servers", yellow("/mcp list"));
    println!("    {}  Add MCP server", yellow("/mcp add <name> <command>"));
    println!("    {}  Remove MCP server", yellow("/mcp remove <name>"));
    println!("    {}  List MCP tools", yellow("/mcp tools"));
    println!("    {}  Generate implementation plan", yellow("/plan <task>"));
    println!("    {}  Brainstorm ideas", yellow("/brainstorm <idea>"));
    println!("    {}  List sessions", yellow("/sessions"));
    println!("    {}  New session", yellow("/new"));
    println!("    {}  Switch session", yellow("/session <id>"));
    println!("    {}  Clear conversation", yellow("/clear"));
    println!("    {}  Exit", yellow("/quit"));
    println!();
    println!("  {}", bold("@ References:"));
    println!("    {}  Include file content in message", cyan("@path/to/file"));
    println!("    {}  Include directory listing", cyan("@path/to/dir"));
    println!();
}

fn print_diff(path: &str, old: &str, new: &str) {
    let diff = FileDiff::diff(old, new, path);
    println!();
    println!("  {} {}", yellow("Diff"), dim(path));
    for line in diff.lines() {
        if line.starts_with('+') && !line.starts_with("+++") {
            println!("    \x1b[32m{}\x1b[0m", line);
        } else if line.starts_with('-') && !line.starts_with("---") {
            println!("    \x1b[31m{}\x1b[0m", line);
        } else if line.starts_with("@@") {
            println!("    \x1b[36m{}\x1b[0m", line);
        } else {
            println!("    {}", line);
        }
    }
}

// ── Main ────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let use_tui = args.contains(&"--tui".to_string());

    let config = load_config()?;
    let model = config.agent.model.clone();

    let llm = Box::new(OpenAiProvider::new(config.api_key.clone(), config.base_url.clone()));
    let tools = default_tools();
    let engine = Arc::new(AgentEngine::new(llm, tools, config.agent.clone()));

    let mut mcp_manager = agent_tools::McpManager::new();

    // Initialize session store
    let session_store = match SessionStore::new() {
        Ok(store) => Some(store),
        Err(_) => None,
    };

    if use_tui {
        // ── TUI mode ────────────────────────────────────────
        let mut tui = tui::TuiApp::new(
            engine,
            session_store,
            mcp_manager,
            &model,
            &std::env::current_dir()?.to_string_lossy(),
        )?;
        tui.run().await?;
    } else {
    // ── REPL mode ─────────────────────────────────────────
    let mut file_snapshots: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut current_session_id = String::new();
    let mut first_user_message: Option<String> = None;

    // Load or create session on startup
    if let Some(ref store) = session_store {
        match store.list() {
            Ok(sessions) if !sessions.is_empty() => {
                current_session_id = sessions[0].id.clone();
                println!("  {} {}", dim("Resumed session:"), dim(&current_session_id[..8]));
            }
            _ => {
                if let Ok(session) = store.create("cli") {
                    current_session_id = session.id.clone();
                    println!("  {} {}", dim("New session:"), dim(&current_session_id[..8]));
                }
            }
        }
    }

    let mut rl = Editor::<AgentHelper, DefaultHistory>::new()?;
    let helper = AgentHelper::new();
    rl.set_helper(Some(helper));
    rl.set_completion_type(rustyline::CompletionType::List);
    rl.set_max_history_size(100)?;

    banner();
    println!("  {} {}", dim("Model:"), dim(&model));
    println!();

    loop {
        let prompt = format!("{} ", cyan(">"));
        let input = match rl.readline(&prompt) {
            Ok(line) => {
                let _ = rl.add_history_entry(&line);
                line
            }
            Err(rustyline::error::ReadlineError::Interrupted) => break,
            Err(rustyline::error::ReadlineError::Eof) => break,
            Err(e) => {
                eprintln!("Error: {}", e);
                break;
            }
        };

        let input = input.trim();
        if input.is_empty() { continue; }

        // ── / commands ─────────────────────────────────────

        if input.starts_with('/') {
            let cmd = input.split_whitespace().next().unwrap_or(input);
            match cmd {
                "/quit" | "/exit" | "/q" => break,
                "/help" | "/h" | "/?" => print_help(),
                "/clear" => {
                    print!("\x1b[2J\x1b[H");
                    io::stdout().flush()?;
                    println!("  {}", dim("Conversation cleared."));
                }
                "/model" => {
                    println!();
                    println!("  {} {}", dim("Model:"), &model);
                    println!("  {} {}", dim("Base URL:"), config.base_url.as_deref().unwrap_or("https://api.openai.com/v1"));
                }
                "/skills" => {
                    println!();
                    let dirs = [
                        (dirs::home_dir().map(|h| h.join(".codex/skills")), "~/.codex/skills"),
                        (dirs::home_dir().map(|h| h.join(".agents/skills")), "~/.agents/skills"),
                        (Some(agent_core::SkillEvolution::default_path()), "~/.agent/skills"),
                    ];
                    for (dir_opt, label) in &dirs {
                        if let Some(dir) = dir_opt {
                            if dir.exists() {
                                println!("  {}", bold(label));
                                if let Ok(entries) = std::fs::read_dir(dir) {
                                    for entry in entries.flatten() {
                                        let name = entry.file_name().to_string_lossy().to_string();
                                        if name.starts_with('.') { continue; }
                                        let has_md = entry.path().join("SKILL.md").exists();
                                        let marker = if has_md { "✓" } else { "○" };
                                        println!("    {} {}", marker, name);
                                    }
                                }
                                println!();
                            }
                        }
                    }
                }
                "/memory" => {
                    println!();
                    println!("  {}", dim("Memory system active. Memories are auto-extracted during conversation."));
                    println!("  {}", dim("Stored at: ~/.agent/memory.db"));
                }
                "/sessions" => {
                    if let Some(ref store) = session_store {
                        match store.list() {
                            Ok(sessions) => {
                                println!();
                                if sessions.is_empty() {
                                    println!("  {}", dim("No sessions found."));
                                } else {
                                    println!("  {}", bold("Sessions:"));
                                    for s in &sessions {
                                        let marker = if s.id == current_session_id { "►" } else { " " };
                                        let title = if s.title.len() > 40 {
                                            format!("{}...", &s.title[..40])
                                        } else {
                                            s.title.clone()
                                        };
                                        println!("    {} {} {} {}", marker, dim(&s.id[..8]), title, dim(&format!("({})", s.message_count)));
                                    }
                                }
                            }
                            Err(e) => println!("  {}", red(&format!("Error: {}", e))),
                        }
                    } else {
                        println!("  {}", dim("Session store not available."));
                    }
                }
                "/new" => {
                    if let Some(ref store) = session_store {
                        match store.create("cli") {
                            Ok(session) => {
                                current_session_id = session.id.clone();
                                first_user_message = None;
                                engine.clear().await;
                                println!();
                                println!("  {} {}", dim("New session:"), dim(&current_session_id[..8]));
                            }
                            Err(e) => println!("  {}", red(&format!("Error: {}", e))),
                        }
                    }
                }
                "/mcp" | "/mcp list" => {
                    println!();
                    let servers = mcp_manager.list_servers();
                    if servers.is_empty() {
                        println!("  {}", dim("No MCP servers connected."));
                    } else {
                        println!("  {}", bold("MCP Servers:"));
                        for name in &servers {
                            println!("    {}", name);
                        }
                    }
                }
                "/mcp tools" => {
                    println!();
                    let tools = mcp_manager.get_all_tools();
                    if tools.is_empty() {
                        println!("  {}", dim("No MCP tools available."));
                    } else {
                        println!("  {}", bold("MCP Tools:"));
                        for tool in &tools {
                            println!("    {} - {}", tool.name(), tool.description());
                        }
                    }
                }
                _ => {
                    if input.starts_with("/mcp add ") {
                        let rest = input[9..].trim();
                        let parts: Vec<&str> = rest.splitn(2, ' ').collect();
                        if parts.len() < 2 {
                            println!("  {}", red("Usage: /mcp add <name> <command> [args...]"));
                        } else {
                            let name = parts[0];
                            let cmd_parts: Vec<&str> = parts[1].split_whitespace().collect();
                            if cmd_parts.is_empty() {
                                println!("  {}", red("Usage: /mcp add <name> <command> [args...]"));
                            } else {
                                let command = cmd_parts[0];
                                let args: Vec<String> = cmd_parts[1..].iter().map(|s| s.to_string()).collect();
                                let env = std::collections::HashMap::new();
                                match mcp_manager.add_stdio(name, command, &args, &env).await {
                                    Ok(()) => println!("  {} {}", green("✓"), format!("Added MCP server '{}'", name)),
                                    Err(e) => println!("  {} {}", red("✗"), format!("Failed to add '{}': {}", name, e)),
                                }
                            }
                        }
                    } else if input.starts_with("/mcp remove ") {
                        let name = input[12..].trim();
                        if name.is_empty() {
                            println!("  {}", red("Usage: /mcp remove <name>"));
                        } else {
                            match mcp_manager.remove(name).await {
                                Ok(()) => println!("  {} {}", green("✓"), format!("Removed MCP server '{}'", name)),
                                Err(e) => println!("  {} {}", red("✗"), format!("Failed to remove '{}': {}", name, e)),
                            }
                        }
                    } else if input.starts_with("/session ") {
                        let id = input[9..].trim();
                        if id.is_empty() {
                            println!("  {}", red("Usage: /session <id>"));
                        } else if let Some(ref store) = session_store {
                            match store.list() {
                                Ok(sessions) => {
                                    if let Some(session) = sessions.iter().find(|s| s.id.starts_with(id)) {
                                        current_session_id = session.id.clone();
                                        first_user_message = None;
                                        engine.clear().await;
                                        println!();
                                        println!("  {} {}", dim("Switched to session:"), dim(&session.id[..8]));
                                        if let Ok(msgs) = store.get_messages(&session.id) {
                                            println!("  {} {}", dim("Messages:"), dim(&msgs.len().to_string()));
                                        }
                                    } else {
                                        println!("  {}", red(&format!("Session '{}' not found.", id)));
                                    }
                                }
                                Err(e) => println!("  {}", red(&format!("Error: {}", e))),
                            }
                        }
                    } else if input.starts_with("/skill create ") {
                        let name = input[14..].trim();
                        if name.is_empty() {
                            println!("  {}", red("Usage: /skill create <name>"));
                        } else {
                            let skills_dir = agent_core::SkillEvolution::default_path();
                            let skill_path = skills_dir.join(name).join("SKILL.md");
                            if let Some(parent) = skill_path.parent() {
                                let _ = std::fs::create_dir_all(parent);
                            }
                            let content = format!(
                                "---\nname: {}\ndescription: Custom skill\nversion: 1\nusage_count: 0\nsuccess_count: 0\n---\n\n# {}\n\n## When to use\n- Describe when to use this skill\n\n## Steps\n1. Step one\n2. Step two\n",
                                name, name
                            );
                            match std::fs::write(&skill_path, &content) {
                                Ok(()) => println!("  {} {}", green("✓"), format!("Created skill '{}' at {}", name, skill_path.display())),
                                Err(e) => println!("  {} {}", red("✗"), format!("Failed to create skill: {}", e)),
                            }
                        }
                    } else if input.starts_with("/skill evolve") {
                        println!();
                        println!("  {}", dim("Analyzing conversation for skill patterns..."));
                        let messages = engine.messages().await;
                        let llm_prov = Arc::new(Box::new(OpenAiProvider::new(config.api_key.clone(), config.base_url.clone())) as Box<dyn agent_llm::LlmProvider>);
                        let se = agent_core::SkillEvolution::new(
                            agent_core::SkillEvolution::default_path().to_str().unwrap_or(".agent/skills"),
                            llm_prov,
                        );
                        match se.maybe_create_skill(&messages, "").await {
                            Some(skill) => {
                                println!("  {} {}", green("✓"), format!("Created skill: {}", skill.name));
                            }
                            None => {
                                println!("  {}", dim("No skill patterns detected yet. Need more tool calls."));
                            }
                        }
                    } else if input.starts_with("/plan ") {
                        let task = input[6..].trim();
                        if task.is_empty() {
                            println!("  {}", red("Usage: /plan <task description>"));
                        } else {
                            println!();
                            println!("  {} {}", yellow("Planning:"), task);
                            print_thinking();
                            let llm_prov = Arc::new(Box::new(OpenAiProvider::new(config.api_key.clone(), config.base_url.clone())) as Box<dyn agent_llm::LlmProvider>);
                            let planner = agent_core::TaskPlanner::new(llm_prov, Some(config.agent.model.clone()));
                            match planner.create_plan(task).await {
                                Ok(plan) => {
                                    println!();
                                    println!("  {} {}", bold("Plan:"), plan.title);
                                    println!("  {}", dim(&plan.overview));
                                    println!();
                                    for step in &plan.steps {
                                        println!("    {} {}", yellow(&format!("{}.", step.id)), &step.title);
                                        println!("      {}", dim(&step.description));
                                    }
                                    if !plan.risks.is_empty() {
                                        println!();
                                        println!("  {}", bold("Risks:"));
                                        for risk in &plan.risks {
                                            println!("    {} {}", yellow("⚠"), risk);
                                        }
                                    }
                                    if !plan.test_strategy.is_empty() {
                                        println!();
                                        println!("  {} {}", bold("Test Strategy:"), dim(&plan.test_strategy));
                                    }
                                }
                                Err(e) => print_error(&format!("Plan generation failed: {}", e)),
                            }
                        }
                    } else if input.starts_with("/brainstorm ") {
                        let idea = input[12..].trim();
                        if idea.is_empty() {
                            println!("  {}", red("Usage: /brainstorm <idea>"));
                        } else {
                            println!();
                            println!("  {} {}", yellow("Brainstorming:"), idea);
                            print_thinking();
                            let llm_prov = Arc::new(Box::new(OpenAiProvider::new(config.api_key.clone(), config.base_url.clone())) as Box<dyn agent_llm::LlmProvider>);
                            let planner = agent_core::TaskPlanner::new(llm_prov, Some(config.agent.model.clone()));
                            match planner.brainstorm(idea).await {
                                Ok(result) => {
                                    println!();
                                    if !result.questions.is_empty() {
                                        println!("  {}", bold("Questions:"));
                                        for q in &result.questions {
                                            println!("    {} {}", yellow("?"), q);
                                        }
                                    }
                                    if !result.suggestions.is_empty() {
                                        println!();
                                        println!("  {}", bold("Suggestions:"));
                                        for s in &result.suggestions {
                                            println!("    {} {}", green("→"), s);
                                        }
                                    }
                                    if !result.alternatives.is_empty() {
                                        println!();
                                        println!("  {}", bold("Alternatives:"));
                                        for a in &result.alternatives {
                                            println!("    {} {}", cyan("◇"), a);
                                        }
                                    }
                                }
                                Err(e) => print_error(&format!("Brainstorm failed: {}", e)),
                            }
                        }
                    } else if input.starts_with("/skill use ") {
                        let args = input[6..].trim();
                        let parts: Vec<&str> = args.splitn(3, ' ').collect();
                        if parts.len() < 2 {
                            println!("  {}", red("Usage: /skill use <name> <task>"));
                        } else {
                            let skill_name = parts[1];
                            let task = if parts.len() > 2 { parts[2] } else { "" };
                            if task.is_empty() {
                                println!("  {}", red("Please specify a task."));
                            } else if let Some(content) = read_skill(skill_name) {
                                let prompt = format!("[Skill: {}]\n{}\n\nTask: {}", skill_name, content, task);
                                println!();
                                println!("  {} /skill use {} {}", cyan("You"), skill_name, task);
                                print_thinking();
                                print_agent_header();
                                match engine.run(&prompt).await {
                                    Ok(response) => {
                                        for line in response.lines() {
                                            print_agent_token(line);
                                            print_agent_newline();
                                        }
                                    }
                                    Err(e) => print_error(&e.to_string()),
                                }
                            } else {
                                println!("  {}", red(&format!("Skill '{}' not found.", skill_name)));
                            }
                        }
                    } else if input.starts_with("/skill ") {
                        let name = input[7..].trim();
                        if name.is_empty() {
                            println!("  {}", red("Usage: /skill <name>"));
                        } else if let Some(content) = read_skill(name) {
                            println!();
                            for line in content.lines().take(60) {
                                println!("  {}", line);
                            }
                            if content.lines().count() > 60 {
                                println!("  {}", dim(&format!("... ({} more lines)", content.lines().count() - 60)));
                            }
                        } else {
                            println!("  {}", red(&format!("Skill '{}' not found.", name)));
                        }
                    } else {
                        println!("  {}", red(&format!("Unknown command: {}. Type /help", cmd)));
                    }
                }
            }
            println!();
            continue;
        }

        // ── Normal message ─────────────────────────────────

        let expanded = resolve_at_references(input);

        // Track first user message for session title
        if first_user_message.is_none() {
            first_user_message = Some(input.to_string());
        }

        // Save user message to session
        if let Some(ref store) = session_store {
            if !current_session_id.is_empty() {
                let _ = store.append_message(&current_session_id, &SessionMessage {
                    role: "user".to_string(),
                    content: input.to_string(),
                    tool_calls: None,
                    tool_call_id: None,
                    timestamp: Utc::now().to_rfc3339(),
                });
            }
        }

        // Take snapshots of files that might be modified
        file_snapshots.clear();

        print_user(input);
        print_thinking();
        print_agent_header();

        // Run agent with streaming
        let engine_clone = engine.clone();
        let mut stream = engine_clone.run_stream(&expanded).await;
        let mut agent_response = String::new();
        let mut tool_snapshots: Vec<(String, String)> = Vec::new();

        while let Some(event) = stream.next().await {
            match event {
                Ok(StreamEvent::Token(token)) => {
                    agent_response.push_str(&token);
                    print_agent_token(&token);
                }
                Ok(StreamEvent::ToolCallStart { name, .. }) => {
                    print_agent_newline();
                    print_tool_start(&name);
                }
                Ok(StreamEvent::ToolSnapshot { path, content }) => {
                    // Snapshot taken BEFORE tool execution (in engine), safe for diff
                    tool_snapshots.push((path, content));
                }
                Ok(StreamEvent::Done) => break,
                Ok(StreamEvent::Error(e)) => {
                    print_agent_newline();
                    print_error(&e);
                    break;
                }
                Err(e) => {
                    print_agent_newline();
                    print_error(&e.to_string());
                    break;
                }
                _ => {}
            }
        }

        // Show diffs for file changes
        for (path, old_content) in &tool_snapshots {
            if let Ok(new_content) = std::fs::read_to_string(path) {
                if old_content != &new_content {
                    print_diff(path, old_content, &new_content);
                }
            }
        }

        // Save agent response to session
        if let Some(ref store) = session_store {
            if !current_session_id.is_empty() && !agent_response.is_empty() {
                let _ = store.append_message(&current_session_id, &SessionMessage {
                    role: "assistant".to_string(),
                    content: agent_response.clone(),
                    tool_calls: None,
                    tool_call_id: None,
                    timestamp: Utc::now().to_rfc3339(),
                });
            }
        }

        println!();
        println!();
    }

    // Update session title on exit
    if let Some(ref store) = session_store {
        if !current_session_id.is_empty() {
            if let Some(ref msg) = first_user_message {
                let title = if msg.len() > 50 {
                    format!("{}...", &msg[..50])
                } else {
                    msg.clone()
                };
                let _ = store.update_title(&current_session_id, &title);
            }
        }
    }

    println!();
    println!("  {}", dim("Bye!"));
    } // end REPL else

    Ok(())
}
