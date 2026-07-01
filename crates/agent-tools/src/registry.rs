use crate::Tool;
use agent_llm::ToolDefinition;
use std::collections::HashMap;

pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn register_raw(&mut self, name: String, tool: Box<dyn Tool>) {
        self.tools.insert(name, tool);
    }

    pub async fn execute(
        &self,
        name: &str,
        args: &serde_json::Value,
        working_dir: &str,
    ) -> anyhow::Result<String> {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("Tool not found: {}", name))?;

        tool.execute(args, working_dir).await
    }

    pub fn get_definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .values()
            .map(|t| ToolDefinition {
                name: t.name().to_string(),
                description: t.description().to_string(),
                parameters: t.input_schema(),
            })
            .collect()
    }

    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    pub fn tool_names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }
}
