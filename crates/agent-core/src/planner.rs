use agent_llm::{ChatRequest, ChatResponse, LlmProvider, Message};
use anyhow::Result;
use std::sync::Arc;

pub struct TaskPlanner {
    llm: Arc<Box<dyn LlmProvider>>,
    model: String,
}

pub struct BrainstormResult {
    pub questions: Vec<String>,
    pub suggestions: Vec<String>,
    pub alternatives: Vec<String>,
}

pub struct ImplementationPlan {
    pub title: String,
    pub overview: String,
    pub steps: Vec<PlanStep>,
    pub risks: Vec<String>,
    pub test_strategy: String,
}

pub struct PlanStep {
    pub id: usize,
    pub title: String,
    pub description: String,
    pub files_to_modify: Vec<String>,
    pub estimated_minutes: usize,
    pub dependencies: Vec<usize>,
}

pub struct SubTask {
    pub title: String,
    pub prompt: String,
    pub context: String,
    pub verification: String,
}

impl TaskPlanner {
    pub fn new(llm: Arc<Box<dyn LlmProvider>>, model: Option<String>) -> Self {
        Self { llm, model: model.unwrap_or_else(|| "gpt-4".to_string()) }
    }

    pub async fn brainstorm(&self, idea: &str) -> Result<BrainstormResult> {
        let prompt = format!(
            "You are a technical brainstorming assistant. The user has an idea:\n\n\
             \"{idea}\"\n\n\
             Provide your response as a JSON object with these keys:\n\
             - questions: array of 3-5 clarifying questions to better understand the requirement\n\
             - suggestions: array of 2-3 actionable suggestions to improve the idea\n\
             - alternatives: array of 2-3 alternative approaches\n\n\
             Return ONLY valid JSON, no markdown fences."
        );

        let request = ChatRequest {
            model: self.model.clone(),
            messages: vec![Message::system(&prompt)],
            tools: vec![],
            temperature: 0.7,
            max_tokens: 1500,
        };

        let response = self.llm.chat(request).await?;
        let text = match response {
            ChatResponse::Text(t) => t,
            _ => return Err(anyhow::anyhow!("Unexpected response type from LLM")),
        };

        let json_str = extract_json(&text);
        let value: serde_json::Value = serde_json::from_str(json_str)
            .map_err(|e| anyhow::anyhow!("Failed to parse brainstorm response: {}", e))?;

        Ok(BrainstormResult {
            questions: parse_string_array(value.get("questions")),
            suggestions: parse_string_array(value.get("suggestions")),
            alternatives: parse_string_array(value.get("alternatives")),
        })
    }

    pub async fn create_plan(&self, design: &str) -> Result<ImplementationPlan> {
        let prompt = format!(
            "You are a technical planning assistant. Based on the following design, \
             create a detailed implementation plan.\n\n\
             Design:\n{design}\n\n\
             Return a JSON object with these keys:\n\
             - title: string, plan title\n\
             - overview: string, 2-3 sentence overview\n\
             - steps: array of objects, each with:\n\
               - id: integer (1-based)\n\
               - title: string\n\
               - description: string\n\
               - files_to_modify: array of file paths\n\
               - estimated_minutes: integer\n\
               - dependencies: array of step ids this depends on\n\
             - risks: array of risk strings\n\
             - test_strategy: string describing how to test\n\n\
             Return ONLY valid JSON, no markdown fences."
        );

        let request = ChatRequest {
            model: self.model.clone(),
            messages: vec![Message::system(&prompt)],
            tools: vec![],
            temperature: 0.4,
            max_tokens: 3000,
        };

        let response = self.llm.chat(request).await?;
        let text = match response {
            ChatResponse::Text(t) => t,
            _ => return Err(anyhow::anyhow!("Unexpected response type from LLM")),
        };

        let json_str = extract_json(&text);
        let value: serde_json::Value = serde_json::from_str(json_str)
            .map_err(|e| anyhow::anyhow!("Failed to parse plan response: {}", e))?;

        let steps = value
            .get("steps")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|s| {
                        Some(PlanStep {
                            id: s.get("id")?.as_u64()? as usize,
                            title: s.get("title")?.as_str()?.to_string(),
                            description: s.get("description")?.as_str()?.to_string(),
                            files_to_modify: parse_string_array(s.get("files_to_modify")),
                            estimated_minutes: s.get("estimated_minutes")?.as_u64()? as usize,
                            dependencies: s
                                .get("dependencies")
                                .and_then(|v| v.as_array())
                                .map(|arr| arr.iter().filter_map(|v| v.as_u64().map(|n| n as usize)).collect())
                                .unwrap_or_default(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(ImplementationPlan {
            title: value
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Untitled Plan")
                .to_string(),
            overview: value
                .get("overview")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            steps,
            risks: parse_string_array(value.get("risks")),
            test_strategy: value
                .get("test_strategy")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        })
    }

    pub async fn decompose(&self, plan: &ImplementationPlan) -> Result<Vec<SubTask>> {
        let steps_desc: Vec<String> = plan
            .steps
            .iter()
            .map(|s| {
                format!(
                    "Step {}: {} - {} (files: {}, deps: {:?})",
                    s.id,
                    s.title,
                    s.description,
                    s.files_to_modify.join(", "),
                    s.dependencies
                )
            })
            .collect();

        let prompt = format!(
            "You are a task decomposition assistant. Convert the following implementation \
             plan steps into independent subtasks that can be executed by coding agents.\n\n\
             Plan: {title}\n{overview}\n\n\
             Steps:\n{steps}\n\n\
             For each step, return a JSON array of objects with:\n\
             - title: string\n\
             - prompt: a detailed prompt for a coding agent to execute this task\n\
             - context: what context/dependencies the agent needs\n\
             - verification: how to verify the task is complete\n\n\
             Return ONLY valid JSON array, no markdown fences.",
            title = plan.title,
            overview = plan.overview,
            steps = steps_desc.join("\n")
        );

        let request = ChatRequest {
            model: self.model.clone(),
            messages: vec![Message::system(&prompt)],
            tools: vec![],
            temperature: 0.3,
            max_tokens: 3000,
        };

        let response = self.llm.chat(request).await?;
        let text = match response {
            ChatResponse::Text(t) => t,
            _ => return Err(anyhow::anyhow!("Unexpected response type from LLM")),
        };

        let json_str = extract_json_array(&text);
        let arr: Vec<serde_json::Value> = serde_json::from_str(json_str)
            .map_err(|e| anyhow::anyhow!("Failed to parse decomposition response: {}", e))?;

        Ok(arr
            .into_iter()
            .filter_map(|v| {
                Some(SubTask {
                    title: v.get("title")?.as_str()?.to_string(),
                    prompt: v.get("prompt")?.as_str()?.to_string(),
                    context: v
                        .get("context")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    verification: v
                        .get("verification")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                })
            })
            .collect())
    }

    pub fn validate_plan(&self, plan: &ImplementationPlan) -> Vec<String> {
        let mut issues = Vec::new();

        if plan.title.is_empty() {
            issues.push("Plan title is empty".to_string());
        }
        if plan.overview.is_empty() {
            issues.push("Plan overview is empty".to_string());
        }
        if plan.steps.is_empty() {
            issues.push("Plan has no steps".to_string());
        }

        let step_ids: Vec<usize> = plan.steps.iter().map(|s| s.id).collect();
        for step in &plan.steps {
            for dep in &step.dependencies {
                if !step_ids.contains(dep) {
                    issues.push(format!(
                        "Step {} depends on non-existent step {}",
                        step.id, dep
                    ));
                }
            }
            if step.description.is_empty() {
                issues.push(format!("Step {} has empty description", step.id));
            }
        }

        for (i, step) in plan.steps.iter().enumerate() {
            for other in &plan.steps[i + 1..] {
                if step.dependencies.contains(&other.id) && other.dependencies.contains(&step.id) {
                    issues.push(format!(
                        "Circular dependency between steps {} and {}",
                        step.id, other.id
                    ));
                }
            }
        }

        issues
    }
}

fn parse_string_array(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn extract_json(text: &str) -> &str {
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            return &text[start..=end];
        }
    }
    text
}

fn extract_json_array(text: &str) -> &str {
    if let Some(start) = text.find('[') {
        if let Some(end) = text.rfind(']') {
            return &text[start..=end];
        }
    }
    "[]"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json() {
        let text = "Here is the result: {\"key\": \"value\"} done.";
        assert_eq!(extract_json(text), "{\"key\": \"value\"}");
    }

    #[test]
    fn test_validate_plan_empty_title() {
        let llm: Arc<Box<dyn LlmProvider>> = Arc::new(Box::new(DummyLlm));
        let planner = TaskPlanner::new(llm);
        let plan = ImplementationPlan {
            title: String::new(),
            overview: "test".to_string(),
            steps: vec![],
            risks: vec![],
            test_strategy: String::new(),
        };
        let issues = planner.validate_plan(&plan);
        assert!(issues.iter().any(|i| i.contains("title")));
    }

    #[test]
    fn test_validate_plan_circular_deps() {
        let llm: Arc<Box<dyn LlmProvider>> = Arc::new(Box::new(DummyLlm));
        let planner = TaskPlanner::new(llm);
        let plan = ImplementationPlan {
            title: "Test".to_string(),
            overview: "Test".to_string(),
            steps: vec![
                PlanStep {
                    id: 1,
                    title: "A".to_string(),
                    description: "A".to_string(),
                    files_to_modify: vec![],
                    estimated_minutes: 10,
                    dependencies: vec![2],
                },
                PlanStep {
                    id: 2,
                    title: "B".to_string(),
                    description: "B".to_string(),
                    files_to_modify: vec![],
                    estimated_minutes: 10,
                    dependencies: vec![1],
                },
            ],
            risks: vec![],
            test_strategy: String::new(),
        };
        let issues = planner.validate_plan(&plan);
        assert!(issues.iter().any(|i| i.contains("Circular")));
    }

    use async_trait::async_trait;
    struct DummyLlm;

    #[async_trait]
    impl LlmProvider for DummyLlm {
        async fn chat(&self, _request: ChatRequest) -> anyhow::Result<ChatResponse> {
            Ok(ChatResponse::Text("{}".to_string()))
        }
    }
}
