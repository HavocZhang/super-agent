use crate::Tool;
use crate::util::resolve_path;
use async_trait::async_trait;
use regex::Regex;
use serde_json::Value;
use std::path::Path;
use walkdir::WalkDir;

pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Search for a pattern in files using regex"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "The regex pattern to search for"
                },
                "path": {
                    "type": "string",
                    "description": "Directory or file to search in (defaults to current directory)"
                },
                "include": {
                    "type": "string",
                    "description": "File extension filter (e.g. '*.rs', '*.toml')"
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, args: &Value, working_dir: &str) -> anyhow::Result<String> {
        let pattern = args["pattern"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'pattern' argument"))?;
        let path = args["path"].as_str().unwrap_or(".");
        let include = args["include"].as_str();

        let re = Regex::new(pattern)
            .map_err(|e| anyhow::anyhow!("Invalid regex '{}': {}", pattern, e))?;

        let resolved = resolve_path(path, working_dir);
        let root = Path::new(&resolved);
        let mut results = Vec::new();

        if root.is_file() {
            search_file(root, &re, include, &mut results);
        } else {
            for entry in WalkDir::new(root)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
            {
                let file_path = entry.path();
                if let Some(ext_filter) = include {
                    let glob_pattern = glob::Pattern::new(ext_filter)
                        .map_err(|e| anyhow::anyhow!("Invalid include pattern '{}': {}", ext_filter, e))?;
                    let file_name = file_path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if !glob_pattern.matches(&file_name) {
                        continue;
                    }
                }
                search_file(file_path, &re, None, &mut results);
            }
        }

        if results.is_empty() {
            Ok(format!("No matches found for '{}'", pattern))
        } else {
            Ok(results.join("\n"))
        }
    }
}

fn search_file(path: &Path, re: &Regex, _include: Option<&str>, results: &mut Vec<String>) {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return,
    };

    for (line_num, line) in content.lines().enumerate() {
        if re.is_match(line) {
            results.push(format!(
                "{}:{}: {}",
                path.display(),
                line_num + 1,
                line
            ));
        }
    }
}
