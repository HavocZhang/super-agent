use agent_llm::ToolCall;
use agent_tools::ToolRegistry;
use std::sync::Arc;
use tracing::info;

pub struct ToolExecutor {
    registry: Arc<ToolRegistry>,
}

impl ToolExecutor {
    pub fn new(registry: Arc<ToolRegistry>) -> Self {
        Self { registry }
    }

    pub async fn execute_batch(&self, calls: &[ToolCall], working_dir: &str) -> Vec<ToolResult> {
        if calls.is_empty() {
            return vec![];
        }

        let groups = partition_parallel(calls);
        let mut all_results: Vec<ToolResult> = Vec::with_capacity(calls.len());

        for group in groups {
            if group.len() == 1 {
                let tc = &calls[group[0]];
                let result = self.execute_single(tc, working_dir).await;
                all_results.push(result);
            } else {
                let handles: Vec<_> = group
                    .iter()
                    .map(|&idx| {
                        let tc = calls[idx].clone();
                        let registry = Arc::clone(&self.registry);
                        let wd = working_dir.to_string();
                        tokio::spawn(async move {
                            let arguments_hash = compute_arguments_hash(&tc.arguments);
                            let output = registry
                                .execute(&tc.name, &tc.arguments, &wd)
                                .await;
                            ToolResult {
                                tool_call_id: tc.id,
                                name: tc.name,
                                arguments: tc.arguments,
                                arguments_hash,
                                output: match output {
                                    Ok(o) => o,
                                    Err(e) => format!("Error: {}", e),
                                },
                            }
                        })
                    })
                    .collect();

                for handle in handles {
                    match handle.await {
                        Ok(result) => all_results.push(result),
                        Err(e) => all_results.push(ToolResult {
                            tool_call_id: String::new(),
                            name: "unknown".to_string(),
                            arguments: serde_json::json!({}),
                            arguments_hash: String::new(),
                            output: format!("Task panicked: {}", e),
                        }),
                    }
                }
            }
        }

        all_results
    }

    async fn execute_single(&self, tc: &ToolCall, working_dir: &str) -> ToolResult {
        info!("Executing tool: {}", tc.name);
        let output = match self
            .registry
            .execute(&tc.name, &tc.arguments, working_dir)
            .await
        {
            Ok(o) => o,
            Err(e) => format!("Error: {}", e),
        };
        info!("Tool {} done ({} bytes)", tc.name, output.len());
        ToolResult {
            tool_call_id: tc.id.clone(),
            name: tc.name.clone(),
            arguments_hash: compute_arguments_hash(&tc.arguments),
            arguments: tc.arguments.clone(),
            output,
        }
    }
}

#[derive(Debug)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub name: String,
    pub arguments: serde_json::Value,
    pub arguments_hash: String,
    pub output: String,
}

fn compute_arguments_hash(args: &serde_json::Value) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    args.to_string().hash(&mut h);
    format!("{:x}", h.finish())
}

fn is_parallelizable(name: &str) -> bool {
    matches!(name, "file_read" | "grep" | "glob" | "ls" | "git_status" | "git_diff")
}

fn is_exclusive(name: &str) -> bool {
    matches!(name, "file_write" | "file_edit" | "shell" | "git_commit")
}

fn partition_parallel(calls: &[ToolCall]) -> Vec<Vec<usize>> {
    if calls.is_empty() {
        return vec![];
    }

    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut current_parallel: Vec<usize> = Vec::new();

    for (i, tc) in calls.iter().enumerate() {
        if is_exclusive(&tc.name) {
            if !current_parallel.is_empty() {
                groups.push(std::mem::take(&mut current_parallel));
            }
            groups.push(vec![i]);
        } else if is_parallelizable(&tc.name) {
            current_parallel.push(i);
        } else {
            if !current_parallel.is_empty() {
                groups.push(std::mem::take(&mut current_parallel));
            }
            groups.push(vec![i]);
        }
    }

    if !current_parallel.is_empty() {
        groups.push(current_parallel);
    }

    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tc(id: &str, name: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            arguments: serde_json::json!({}),
        }
    }

    #[test]
    fn test_partition_all_parallel() {
        let calls = vec![
            make_tc("1", "file_read"),
            make_tc("2", "grep"),
            make_tc("3", "glob"),
        ];
        let groups = partition_parallel(&calls);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0], vec![0, 1, 2]);
    }

    #[test]
    fn test_partition_exclusive_breaks() {
        let calls = vec![
            make_tc("1", "file_read"),
            make_tc("2", "grep"),
            make_tc("3", "shell"),
            make_tc("4", "file_read"),
        ];
        let groups = partition_parallel(&calls);
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0], vec![0, 1]);
        assert_eq!(groups[1], vec![2]);
        assert_eq!(groups[2], vec![3]);
    }

    #[test]
    fn test_partition_write_not_parallel() {
        let calls = vec![
            make_tc("1", "file_write"),
            make_tc("2", "file_edit"),
        ];
        let groups = partition_parallel(&calls);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0], vec![0]);
        assert_eq!(groups[1], vec![1]);
    }
}
