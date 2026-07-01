use crate::{AgentConfig, AgentEngine};
use agent_tools::default_tools;
use anyhow::Result;
use std::sync::Arc;
use tracing::{debug, info};

#[derive(Debug, Clone)]
pub enum SubagentType {
    Explore,
    General,
}

pub struct SubagentTask {
    pub task_type: SubagentType,
    pub prompt: String,
    pub context: String,
    pub max_iterations: usize,
}

impl SubagentTask {
    pub fn explore(prompt: impl Into<String>, context: impl Into<String>) -> Self {
        Self {
            task_type: SubagentType::Explore,
            prompt: prompt.into(),
            context: context.into(),
            max_iterations: 10,
        }
    }

    pub fn general(prompt: impl Into<String>, context: impl Into<String>) -> Self {
        Self {
            task_type: SubagentType::General,
            prompt: prompt.into(),
            context: context.into(),
            max_iterations: 10,
        }
    }
}

#[derive(Debug)]
pub struct SubagentResult {
    pub status: String,
    pub summary: String,
    pub files_touched: Vec<String>,
    pub findings: Vec<String>,
}

pub struct SubagentManager {
    engine: Arc<AgentEngine>,
}

impl SubagentManager {
    pub fn new(engine: Arc<AgentEngine>) -> Self {
        Self { engine }
    }

    pub async fn run(&self, task: SubagentTask) -> Result<SubagentResult> {
        let tools = match task.task_type {
            SubagentType::Explore => readonly_tools(),
            SubagentType::General => default_tools(),
        };

        let system_prompt = match task.task_type {
            SubagentType::Explore => {
                "You are an explore subagent. You can ONLY read files and search code. \
                 You cannot write files or run shell commands. \
                 Return concise findings with file paths and line numbers. \
                 When done, provide a clear summary of what you found."
            }
            SubagentType::General => {
                "You are a general-purpose subagent. Complete the assigned task efficiently. \
                 When done, provide a clear summary of what you did and any files you touched."
            }
        };

        let config = AgentConfig {
            system_prompt: system_prompt.to_string(),
            model: self.engine.config().model.clone(),
            temperature: self.engine.config().temperature,
            max_tokens: self.engine.config().max_tokens,
            max_iterations: task.max_iterations,
            working_dir: self.engine.working_dir().await,
            permission_mode: "default".to_string(),
            context_max_tokens: self.engine.config().context_max_tokens,
        };

        let llm = self.engine.llm_clone();
        let sub_engine = AgentEngine::from_parts(llm, tools, config);

        let mut prompt = String::new();
        if !task.context.is_empty() {
            prompt.push_str(&format!("Context:\n{}\n\n", task.context));
        }
        prompt.push_str(&task.prompt);

        info!("Subagent {:?} starting: {}", task.task_type, task.prompt);
        let result = sub_engine.run(&prompt).await;

        match result {
            Ok(response) => {
                let files = extract_file_paths(&response);
                let findings = extract_findings(&response);
                info!("Subagent completed successfully");
                Ok(SubagentResult {
                    status: "success".to_string(),
                    summary: response,
                    files_touched: files,
                    findings,
                })
            }
            Err(e) => {
                debug!("Subagent error: {}", e);
                Ok(SubagentResult {
                    status: "failed".to_string(),
                    summary: format!("Subagent failed: {}", e),
                    files_touched: vec![],
                    findings: vec![],
                })
            }
        }
    }

    pub async fn run_parallel(&self, tasks: Vec<SubagentTask>) -> Result<Vec<SubagentResult>> {
        let mut handles = Vec::with_capacity(tasks.len());

        for task in tasks {
            let manager = SubagentManager::new(Arc::clone(&self.engine));
            handles.push(tokio::spawn(async move { manager.run(task).await }));
        }

        let mut results = Vec::with_capacity(handles.len());
        for handle in handles {
            match handle.await? {
                Ok(result) => results.push(result),
                Err(e) => results.push(SubagentResult {
                    status: "failed".to_string(),
                    summary: format!("Task panicked: {}", e),
                    files_touched: vec![],
                    findings: vec![],
                }),
            }
        }

        Ok(results)
    }
}

fn readonly_tools() -> agent_tools::ToolRegistry {
    agent_tools::readonly_tools()
}

fn extract_file_paths(text: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("src/") || trimmed.starts_with("crates/") || trimmed.starts_with("./")
            || (trimmed.contains(".rs") || trimmed.contains(".ts") || trimmed.contains(".py"))
        {
            let path = trimmed.split_whitespace().next().unwrap_or(trimmed);
            let path = path.trim_end_matches(':').trim_end_matches(',');
            if !path.is_empty() && path.len() < 200 {
                paths.push(path.to_string());
            }
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

fn extract_findings(text: &str) -> Vec<String> {
    let mut findings = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("• ") {
            findings.push(trimmed[2..].trim().to_string());
        }
    }
    findings
}
