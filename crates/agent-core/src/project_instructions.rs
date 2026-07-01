use std::path::{Path, PathBuf};

const PROJECT_FILENAMES: &[&str] = &["AGENTS.md", "CLAUDE.md"];
const GLOBAL_FILENAMES: &[&str] = &["AGENTS.md"];
const PROJECT_AGENT_DIR_FILENAME: &str = "rules.md";
const GLOBAL_RULES_DIR: &str = "rules";
const MAX_DEPTH: usize = 5;

pub struct ProjectInstructions;

impl ProjectInstructions {
    pub fn load(working_dir: &str) -> Option<String> {
        let mut sections: Vec<String> = Vec::new();

        if let Some(global_content) = load_global_instructions() {
            sections.push(global_content);
        }

        let global_rules = load_global_rules();
        for rule in global_rules {
            sections.push(rule);
        }

        let start = Path::new(working_dir);
        let mut project_root: Option<&Path> = None;
        let mut current = Some(start);

        for _ in 0..MAX_DEPTH {
            if let Some(dir) = current {
                if let Some(content) = try_load_project_file(dir) {
                    project_root = Some(dir);
                    sections.push(content);
                    break;
                }
                current = dir.parent();
            } else {
                break;
            }
        }

        current = Some(start);
        for _ in 0..MAX_DEPTH {
            if let Some(dir) = current {
                if let Some(content) = try_load_project_agent_rules(dir) {
                    sections.push(content);
                }
                if Some(dir) == project_root {
                    break;
                }
                current = dir.parent();
            } else {
                break;
            }
        }

        if sections.is_empty() {
            None
        } else {
            Some(sections.join("\n\n"))
        }
    }
}

fn global_agent_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".agent"))
}

fn load_global_instructions() -> Option<String> {
    let agent_dir = global_agent_dir()?;
    for filename in GLOBAL_FILENAMES {
        let path = agent_dir.join(filename);
        if let Ok(content) = std::fs::read_to_string(&path) {
            if !content.trim().is_empty() {
                return Some(content);
            }
        }
    }
    None
}

fn load_global_rules() -> Vec<String> {
    let agent_dir = match global_agent_dir() {
        Some(d) => d,
        None => return vec![],
    };

    let rules_dir = agent_dir.join(GLOBAL_RULES_DIR);
    if !rules_dir.exists() || !rules_dir.is_dir() {
        return vec![];
    }

    let mut rules = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&rules_dir) {
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .filter(|e| {
                e.path()
                    .extension()
                    .map(|ext| ext == "md")
                    .unwrap_or(false)
            })
            .map(|e| e.path())
            .collect();
        files.sort();

        for path in files {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if !content.trim().is_empty() {
                    rules.push(content);
                }
            }
        }
    }

    rules
}

fn try_load_project_file(dir: &Path) -> Option<String> {
    for filename in PROJECT_FILENAMES {
        let path = dir.join(filename);
        if let Ok(content) = std::fs::read_to_string(&path) {
            if !content.trim().is_empty() {
                return Some(content);
            }
        }
    }
    None
}

fn try_load_project_agent_rules(dir: &Path) -> Option<String> {
    let path = dir.join(".agent").join(PROJECT_AGENT_DIR_FILENAME);
    std::fs::read_to_string(&path)
        .ok()
        .filter(|c| !c.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_load_agents_md() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("AGENTS.md"), "use cargo test").unwrap();

        let result = ProjectInstructions::load(dir.path().to_str().unwrap());
        let content = result.unwrap();
        assert!(content.contains("use cargo test"));
    }

    #[test]
    fn test_agents_md_takes_priority_over_claude() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("AGENTS.md"), "agents content").unwrap();
        fs::write(dir.path().join("CLAUDE.md"), "claude content").unwrap();

        let result = ProjectInstructions::load(dir.path().to_str().unwrap());
        let content = result.unwrap();
        assert!(content.contains("agents content"));
        assert!(!content.contains("claude content"));
    }

    #[test]
    fn test_falls_back_to_claude_md() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("CLAUDE.md"), "claude content").unwrap();

        let result = ProjectInstructions::load(dir.path().to_str().unwrap());
        let content = result.unwrap();
        assert!(content.contains("claude content"));
    }

    #[test]
    fn test_falls_back_to_agent_rules() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".agent")).unwrap();
        fs::write(dir.path().join(".agent/rules.md"), "agent rules").unwrap();

        let result = ProjectInstructions::load(dir.path().to_str().unwrap());
        let content = result.unwrap();
        assert!(content.contains("agent rules"));
    }

    #[test]
    fn test_searches_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let child = dir.path().join("a").join("b").join("c");
        fs::create_dir_all(&child).unwrap();
        fs::write(dir.path().join("AGENTS.md"), "root instructions").unwrap();

        let result = ProjectInstructions::load(child.to_str().unwrap());
        let content = result.unwrap();
        assert!(content.contains("root instructions"));
    }

    #[test]
    fn test_returns_none_when_no_instructions() {
        let dir = tempfile::tempdir().unwrap();
        let result = ProjectInstructions::load(dir.path().to_str().unwrap());
        assert!(result.is_none());
    }

    #[test]
    fn test_project_rules_merged_from_intermediate_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let mid = dir.path().join("a").join("b");
        fs::create_dir_all(&mid).unwrap();

        fs::write(dir.path().join("AGENTS.md"), "project root").unwrap();

        fs::create_dir(mid.join(".agent")).unwrap();
        fs::write(mid.join(".agent/rules.md"), "mid rules").unwrap();

        let result = ProjectInstructions::load(mid.to_str().unwrap());
        let content = result.unwrap();
        assert!(content.contains("project root"));
        assert!(content.contains("mid rules"));
    }
}
