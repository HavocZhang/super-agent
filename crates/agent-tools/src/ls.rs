use crate::Tool;
use async_trait::async_trait;
use serde_json::Value;
use std::path::Path;

pub struct LsTool;

#[async_trait]
impl Tool for LsTool {
    fn name(&self) -> &str {
        "ls"
    }

    fn description(&self) -> &str {
        "List directory contents"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path to list (defaults to current directory)"
                },
                "show_hidden": {
                    "type": "boolean",
                    "description": "Whether to show hidden files (defaults to false)"
                }
            },
            "required": []
        })
    }

    async fn execute(&self, args: &Value, working_dir: &str) -> anyhow::Result<String> {
        let path = args["path"].as_str().unwrap_or(".");
        let show_hidden = args["show_hidden"].as_bool().unwrap_or(false);

        let dir_path = if Path::new(path).is_absolute() {
            path.to_string()
        } else {
            Path::new(working_dir).join(path).to_string_lossy().to_string()
        };

        let mut entries = tokio::fs::read_dir(&dir_path).await
            .map_err(|e| anyhow::anyhow!("Failed to read directory '{}': {}", path, e))?;

        let mut names = Vec::new();

        while let Some(entry) = entries.next_entry().await
            .map_err(|e| anyhow::anyhow!("Failed to read entry: {}", e))?
        {
            let file_name = entry.file_name().to_string_lossy().to_string();

            // Skip hidden files unless show_hidden is true
            if !show_hidden && file_name.starts_with('.') {
                continue;
            }

            let file_type = entry.file_type().await.ok();
            let metadata = entry.metadata().await.ok();

            let kind = file_type.as_ref().map_or("file", |ft| {
                if ft.is_dir() {
                    "dir"
                } else if ft.is_symlink() {
                    "symlink"
                } else {
                    "file"
                }
            });

            let size = metadata
                .map(|m| m.len())
                .map(|s| human_size(s))
                .unwrap_or_default();

            names.push(format!("{}  {}  {}", kind, size, file_name));
        }

        names.sort();

        if names.is_empty() {
            Ok(format!("(empty directory: {})", path))
        } else {
            Ok(names.join("\n"))
        }
    }
}

fn human_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;

    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }

    if unit_idx == 0 {
        format!("{} {}", bytes, UNITS[unit_idx])
    } else {
        format!("{:.1} {}", size, UNITS[unit_idx])
    }
}
