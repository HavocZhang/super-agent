use anyhow::Result;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
pub enum HookEvent {
    PreToolUse,
    PostToolUse,
    UserPromptSubmit,
    Stop,
    SessionStart,
    SessionEnd,
    SubagentStop,
    Notification,
    PreCompact,
}

#[derive(Debug, Clone)]
pub struct HookConfig {
    pub event: HookEvent,
    pub command: String,
    pub timeout_ms: u64,
    pub blocking: bool,
}

impl Default for HookConfig {
    fn default() -> Self {
        Self {
            event: HookEvent::PreToolUse,
            command: String::new(),
            timeout_ms: 5000,
            blocking: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum HookResult {
    Pass,
    Block(String),
    Warn(String),
    Timeout,
    Error(String),
}

#[derive(Debug, Clone, Deserialize)]
struct SettingsHook {
    event: HookEvent,
    command: String,
    #[serde(default = "default_timeout")]
    timeout_ms: u64,
    #[serde(default)]
    blocking: bool,
}

fn default_timeout() -> u64 {
    5000
}

#[derive(Debug, Deserialize)]
struct Settings {
    hooks: Option<Vec<SettingsHook>>,
}

pub struct HookExecutor {
    hooks: Vec<HookConfig>,
}

impl Default for HookExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl HookExecutor {
    pub fn new() -> Self {
        Self { hooks: Vec::new() }
    }

    pub fn add_hook(&mut self, config: HookConfig) {
        self.hooks.push(config);
    }

    pub fn load_from_settings(&mut self, project_dir: &str) -> Result<()> {
        let settings_path = Path::new(project_dir).join(".agent/settings.json");
        if !settings_path.exists() {
            return Ok(());
        }

        let content = std::fs::read_to_string(&settings_path)?;
        let settings: Settings = serde_json::from_str(&content)?;

        if let Some(hooks) = settings.hooks {
            for h in hooks {
                self.hooks.push(HookConfig {
                    event: h.event,
                    command: h.command,
                    timeout_ms: h.timeout_ms,
                    blocking: h.blocking,
                });
            }
        }

        Ok(())
    }

    pub async fn execute(&self, event: HookEvent, payload: &serde_json::Value) -> HookResult {
        let hooks: Vec<_> = self.hooks.iter().filter(|h| h.event == event).collect();
        if hooks.is_empty() {
            return HookResult::Pass;
        }

        let payload_str = serde_json::to_string(payload).unwrap_or_default();
        let mut block_result = None;

        for hook in hooks {
            match self.run_one(hook, &payload_str).await {
                HookResult::Pass => continue,
                HookResult::Block(reason) => {
                    if hook.blocking {
                        return HookResult::Block(reason);
                    } else {
                        block_result = Some(HookResult::Warn(reason));
                    }
                }
                other => return other,
            }
        }

        block_result.unwrap_or(HookResult::Pass)
    }

    async fn run_one(&self, hook: &HookConfig, payload: &str) -> HookResult {
        use tokio::io::AsyncWriteExt;
        use tokio::process::Command;

        let timeout = std::time::Duration::from_millis(hook.timeout_ms);

        let mut child = match Command::new("sh")
            .arg("-c")
            .arg(&hook.command)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => return HookResult::Error(format!("Failed to spawn hook: {}", e)),
        };

        if let Some(ref mut stdin) = child.stdin {
            let _ = stdin.write_all(payload.as_bytes()).await;
            drop(child.stdin.take());
        }

        match tokio::time::timeout(timeout, child.wait_with_output()).await {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                match output.status.code() {
                    Some(0) => HookResult::Pass,
                    Some(2) => HookResult::Block(if stdout.is_empty() {
                        "blocked by hook".to_string()
                    } else {
                        stdout
                    }),
                    Some(code) => HookResult::Warn(format!("exit {}: {}", code, stdout)),
                    None => HookResult::Error("Hook terminated by signal".to_string()),
                }
            }
            Ok(Err(e)) => HookResult::Error(format!("Hook error: {}", e)),
            Err(_) => HookResult::Timeout,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_hook_pass() {
        let mut exec = HookExecutor::new();
        exec.add_hook(HookConfig {
            event: HookEvent::PreToolUse,
            command: "echo ok".to_string(),
            timeout_ms: 5000,
            blocking: true,
        });
        let result = exec.execute(HookEvent::PreToolUse, &serde_json::json!({})).await;
        assert_eq!(result, HookResult::Pass);
    }

    #[tokio::test]
    async fn test_hook_block() {
        let mut exec = HookExecutor::new();
        exec.add_hook(HookConfig {
            event: HookEvent::PreToolUse,
            command: "echo blocked >&2; exit 2".to_string(),
            timeout_ms: 5000,
            blocking: true,
        });
        let result = exec.execute(HookEvent::PreToolUse, &serde_json::json!({})).await;
        assert!(matches!(result, HookResult::Block(_)));
    }

    #[tokio::test]
    async fn test_hook_non_blocking() {
        let mut exec = HookExecutor::new();
        exec.add_hook(HookConfig {
            event: HookEvent::PostToolUse,
            command: "exit 2".to_string(),
            timeout_ms: 5000,
            blocking: false,
        });
        let result = exec.execute(HookEvent::PostToolUse, &serde_json::json!({})).await;
        assert!(matches!(result, HookResult::Warn(_)));
    }

    #[tokio::test]
    async fn test_hook_no_hooks() {
        let exec = HookExecutor::new();
        let result = exec.execute(HookEvent::PreToolUse, &serde_json::json!({})).await;
        assert_eq!(result, HookResult::Pass);
    }
}
