use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum PermissionMode {
    Plan,
    Default,
    Auto,
    Yolo,
}

impl PermissionMode {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "plan" => Self::Plan,
            "default" => Self::Default,
            "auto" => Self::Auto,
            "yolo" => Self::Yolo,
            _ => Self::Default,
        }
    }
}

#[derive(Debug, Clone)]
pub enum PermissionAction {
    Allow,
    Deny,
    Ask,
}

#[derive(Debug, Clone)]
pub enum PermissionResult {
    Allowed,
    Denied(String),
    NeedsApproval(String),
}

pub struct PermissionManager {
    mode: PermissionMode,
    rules: Vec<(String, PermissionAction)>,
}

impl PermissionManager {
    pub fn new(mode: PermissionMode) -> Self {
        let rules = Self::default_rules();
        Self { mode, rules }
    }

    fn default_rules() -> Vec<(String, PermissionAction)> {
        vec![
            ("file_read".into(), PermissionAction::Allow),
            ("grep".into(), PermissionAction::Allow),
            ("glob".into(), PermissionAction::Allow),
            ("ls".into(), PermissionAction::Allow),
            ("git_status".into(), PermissionAction::Allow),
            ("git_diff".into(), PermissionAction::Allow),
            ("skill_list".into(), PermissionAction::Allow),
            ("skill_read".into(), PermissionAction::Allow),
            ("shell".into(), PermissionAction::Ask),
            ("file_write".into(), PermissionAction::Ask),
            ("file_edit".into(), PermissionAction::Ask),
            ("git_commit".into(), PermissionAction::Ask),
        ]
    }

    pub fn check(&self, tool: &str, args: &Value) -> PermissionResult {
        match self.mode {
            PermissionMode::Yolo => PermissionResult::Allowed,
            PermissionMode::Plan => {
                if Self::is_read_only(tool) {
                    PermissionResult::Allowed
                } else {
                    PermissionResult::Denied("Plan mode: read-only, no modifications allowed".into())
                }
            }
            PermissionMode::Default | PermissionMode::Auto => {
                if self.mode == PermissionMode::Auto && Self::is_read_only(tool) {
                    return PermissionResult::Allowed;
                }

                let action = self.rules.iter()
                    .find(|(name, _)| name == tool)
                    .map(|(_, action)| action.clone())
                    .unwrap_or(PermissionAction::Ask);

                match action {
                    PermissionAction::Allow => PermissionResult::Allowed,
                    PermissionAction::Deny => PermissionResult::Denied(format!("Tool '{}' is denied by policy", tool)),
                    PermissionAction::Ask => {
                        if tool == "shell" {
                            if let Some(cmd) = args["command"].as_str() {
                                if let Some(reason) = Self::detect_dangerous_command(cmd) {
                                    return PermissionResult::Denied(reason);
                                }
                            }
                        }
                        PermissionResult::NeedsApproval(format!(
                            "Tool '{}' requires approval. Args: {}",
                            tool,
                            serde_json::to_string(args).unwrap_or_default()
                        ))
                    }
                }
            }
        }
    }

    fn is_read_only(tool: &str) -> bool {
        matches!(
            tool,
            "file_read" | "grep" | "glob" | "ls" | "git_status" | "git_diff" | "skill_list" | "skill_read"
        )
    }

    fn detect_dangerous_command(cmd: &str) -> Option<String> {
        let lower = cmd.to_lowercase();
        let dangerous_patterns = [
            ("rm -rf /", "Dangerous: recursive delete from root"),
            ("rm -rf /*", "Dangerous: recursive delete from root"),
            ("rm -rf ~", "Dangerous: recursive delete home directory"),
            ("chmod -r 777 /", "Dangerous: recursive chmod on root"),
            ("chmod 777 /", "Dangerous: chmod on root"),
            ("> /dev/sd", "Dangerous: writing to block device"),
            ("mkfs.", "Dangerous: filesystem formatting"),
            ("dd if=", "Dangerous: raw disk operation"),
            (":(){ :|:& };:", "Dangerous: fork bomb"),
            ("curl|sh", "Dangerous: pipe to shell"),
            ("curl|bash", "Dangerous: pipe to shell"),
            ("wget|sh", "Dangerous: pipe to shell"),
            ("wget|bash", "Dangerous: pipe to shell"),
        ];

        for (pattern, reason) in &dangerous_patterns {
            if lower.contains(pattern) {
                return Some(reason.to_string());
            }
        }

        None
    }

    pub fn set_mode(&mut self, mode: PermissionMode) {
        self.mode = mode;
    }

    pub fn mode(&self) -> &PermissionMode {
        &self.mode
    }
}
