use agent_core::{AgentConfig, AgentEngine};
use agent_llm::OpenAiProvider;
use agent_tools::default_tools;
use anyhow::Result;
use std::io::{self, Write};

#[derive(serde::Deserialize)]
struct AppConfig {
    api_key: String,
    base_url: Option<String>,
    #[serde(default)]
    agent: AgentConfig,
}

fn load_config() -> Result<AppConfig> {
    let config_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".agent")
        .join("config.toml");

    if config_path.exists() {
        let content = std::fs::read_to_string(&config_path)?;
        let config: AppConfig = toml::from_str(&content)?;
        return Ok(config);
    }

    let api_key = std::env::var("OPENAI_API_KEY")
        .or_else(|_| std::env::var("ANTHROPIC_API_KEY"))
        .unwrap_or_else(|_| {
            eprintln!("请设置 OPENAI_API_KEY 环境变量或创建 ~/.agent/config.toml");
            std::process::exit(1);
        });

    Ok(AppConfig {
        api_key,
        base_url: None,
        agent: AgentConfig::default(),
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let config = load_config()?;

    let llm = Box::new(OpenAiProvider::new(
        config.api_key.clone(),
        config.base_url.clone(),
    ));

    let tools = default_tools();
    let engine = AgentEngine::new(llm, tools, config.agent.clone());

    println!("🤖 Agent: 你好！我是你的编码助手，有什么可以帮你的？");
    println!("         输入 /quit 退出");
    println!();

    loop {
        print!("👤 你: ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        if input == "/quit" || input == "/exit" {
            break;
        }

        if input.is_empty() {
            continue;
        }

        println!();
        print!("🤖 Agent: ");
        io::stdout().flush()?;

        match engine.run(input).await {
            Ok(response) => {
                println!("{}", response);
            }
            Err(e) => {
                println!("错误: {}", e);
            }
        }

        println!();
    }

    println!("再见！");
    Ok(())
}
