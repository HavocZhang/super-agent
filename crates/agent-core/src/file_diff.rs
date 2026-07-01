use similar::TextDiff;

#[derive(Debug, Clone)]
pub enum ChangeType {
    Added,
    Modified,
    Deleted,
}

#[derive(Debug, Clone)]
pub struct FileChange {
    pub path: String,
    pub change_type: ChangeType,
    pub diff: Option<String>,
}

pub struct FileDiff;

impl FileDiff {
    pub fn diff(old: &str, new: &str, path: &str) -> String {
        let diff = TextDiff::from_lines(old, new);
        let mut output = String::new();
        output.push_str(&format!("--- a/{}\n", path));
        output.push_str(&format!("+++ b/{}\n", path));

        for hunk in diff.unified_diff().context_radius(3).iter_hunks() {
            output.push_str(&format!("{}\n", hunk));
        }

        output
    }

    pub fn extract_changes(output: &str) -> Vec<FileChange> {
        let mut changes = Vec::new();

        for line in output.lines() {
            if line.contains("Successfully written") || line.contains("Successfully edited") {
                // Try to extract file path from the message
                let path = Self::extract_path_from_success(line);
                if let Some(path) = path {
                    let change_type = if line.contains("Successfully written") {
                        ChangeType::Added
                    } else {
                        ChangeType::Modified
                    };
                    changes.push(FileChange {
                        path,
                        change_type,
                        diff: None,
                    });
                }
            } else if line.contains("Successfully deleted") || line.contains("Deleted file") {
                if let Some(path) = Self::extract_path_from_success(line) {
                    changes.push(FileChange {
                        path,
                        change_type: ChangeType::Deleted,
                        diff: None,
                    });
                }
            }
        }

        changes
    }

    fn extract_path_from_success(line: &str) -> Option<String> {
        // Try patterns like "Successfully written to <path>" or "Successfully edited <path>"
        let patterns = [
            "Successfully written to ",
            "Successfully written: ",
            "Successfully edited ",
            "Successfully edited: ",
            "Successfully deleted ",
            "Deleted file: ",
            "Deleted file ",
        ];

        for pattern in &patterns {
            if let Some(pos) = line.find(pattern) {
                let after = &line[pos + pattern.len()..];
                // Take until end of line or a delimiter
                let path: String = after
                    .chars()
                    .take_while(|c| !c.is_whitespace() && *c != '.' && *c != ',')
                    .collect();
                if !path.is_empty() {
                    return Some(path);
                }
            }
        }

        None
    }

    pub fn format_diff_colored(diff: &str) -> String {
        let mut output = String::new();
        for line in diff.lines() {
            if line.starts_with('+') && !line.starts_with("+++") {
                output.push_str(&format!("\x1b[32m{}\x1b[0m\n", line));
            } else if line.starts_with('-') && !line.starts_with("---") {
                output.push_str(&format!("\x1b[31m{}\x1b[0m\n", line));
            } else if line.starts_with("@@") {
                output.push_str(&format!("\x1b[36m{}\x1b[0m\n", line));
            } else {
                output.push_str(&format!("{}\n", line));
            }
        }
        output
    }
}
