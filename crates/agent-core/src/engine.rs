use crate::AgentConfig;
use agent_llm::{ChatRequest, ChatResponse, LlmProvider, Message};
use agent_tools::ToolRegistry;
use anyhow::Result;
use tracing::{debug, info};

pub struct AgentEngine {
    llm: Box<dyn LlmProvider>,
    tools: ToolRegistry,
    config: AgentConfig,
}

impl AgentEngine {
    pub fn new(llm: Box<dyn LlmProvider>, tools: ToolRegistry, config: AgentConfig) -> Self {
        Self { llm, tools, config }
    }

    pub async fn run(&self, user_message: &str) -> Result<String> {
        let mut messages = vec![
            Message::system(&self.config.system_prompt),
            Message::user(user_message),
        ];

        let tools = self.tools.get_definitions();

        for iteration in 0..self.config.max_iterations {
            debug!(
                "Iteration {}/{}",
                iteration + 1,
                self.config.max_iterations
            );

            let request = ChatRequest {
                model: self.config.model.clone(),
                messages: messages.clone(),
                tools: tools.clone(),
                temperature: self.config.temperature,
                max_tokens: self.config.max_tokens,
            };

            let response = self.llm.chat(request).await?;

            match response {
                ChatResponse::Text(text) => {
                    info!("Agent completed with text response");
                    return Ok(text);
                }
                ChatResponse::ToolCall(tool_calls) => {
                    messages.push(Message::assistant_with_tool_calls(tool_calls.clone()));

                    for tool_call in &tool_calls {
                        info!("Executing tool: {}", tool_call.name);
                        println!("  [调用工具: {}]", tool_call.name);

                        let result = self
                            .tools
                            .execute(&tool_call.name, &tool_call.arguments)
                            .await;

                        let output = match result {
                            Ok(output) => output,
                            Err(e) => format!("Error: {}", e),
                        };

                        let display = if output.len() > 500 {
                            format!("{}...", &output[..500])
                        } else {
                            output.clone()
                        };
                        println!("  [结果: {}]", display);

                        messages.push(Message::tool_result(&tool_call.id, &output));
                    }
                }
            }
        }

        Err(anyhow::anyhow!(
            "Max iterations ({}) reached",
            self.config.max_iterations
        ))
    }
}
