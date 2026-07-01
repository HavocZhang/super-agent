use crate::Tool;
use crate::util::resolve_path;
use async_trait::async_trait;
use serde_json::Value;

pub struct GlobTool;

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        "glob"
    }

    fn description(&self) -> &str {
        "Find files matching a glob pattern"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern (e.g., '**/*.rs', '*.toml')"
                },
                "path": {
                    "type": "string",
                    "description": "Directory to search in (defaults to current directory)"
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

        let resolved = resolve_path(path, working_dir);

        let full_pattern = if resolved.ends_with('/') {
            format!("{}{}", resolved, pattern)
        } else {
            format!("{}/{}", resolved, pattern)
        };

        let paths = glob::glob(&full_pattern)
            .map_err(|e| anyhow::anyhow!("Invalid glob pattern '{}': {}", full_pattern, e))?;

        let mut results = Vec::new();
        for entry in paths {
            match entry {
                Ok(p) => results.push(p.display().to_string()),
                Err(e) => results.push(format!("Error: {}", e)),
            }
        }

        if results.is_empty() {
            Ok(format!("No files found matching '{}'", pattern))
        } else {
            Ok(results.join("\n"))
        }
    }
}
