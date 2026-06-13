use std::sync::Arc;

use rust_agent_client::{
    ChatClientConfig, DeepSeekChatClient, OpenAiChatClient,
};
use rust_agent_core::{
    AgentId, AgentSession, ChatMessage, IAgent, ISession, IWorkflow,
    ToolRegistry, collect_agent_response,
};
use rust_agent_framework::tool;
use rust_agent_framework::ChatClientAgent;
use rust_agent_workflow::GraphFlow;
use tracing_subscriber::EnvFilter;

// Define tools with the #[tool] macro — minimal boilerplate
#[tool(description = "Echoes back the input text")]
async fn echo(#[param(desc = "The text to echo")] text: String) -> String {
    text
}

#[tool(description = "Adds two numbers together")]
async fn add(#[param(desc = "First number")] a: i64, #[param(desc = "Second number")] b: i64) -> String {
    format!("{}", a + b)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    // ---- Demo 1: OpenAI Chat Client ----
    tracing::info!("=== OpenAI Demo ===");
    let oai_config = ChatClientConfig::openai("gpt-4.1-mini", std::env::var("OPENAI_API_KEY").unwrap_or_default());
    let oai_client = OpenAiChatClient::new(oai_config)?;

    // List available models (provider-specific API)
    match oai_client.list_models().await {
        Ok(models) => {
            tracing::info!("OpenAI models: {}",
                models.iter().take(5).map(|m| m.id.as_str()).collect::<Vec<_>>().join(", "));
        }
        Err(e) => tracing::warn!("list_models failed (check API key): {}", e),
    }

    // ---- Demo 2: DeepSeek Chat Client with Thinking Mode ----
    tracing::info!("=== DeepSeek Demo ===");
    let ds_config = ChatClientConfig::deepseek(
        "deepseek-v4-pro",
        std::env::var("DEEPSEEK_API_KEY").unwrap_or_default(),
    );
    let mut ds_client = DeepSeekChatClient::new(ds_config)?;
    ds_client.enable_thinking(true);
    ds_client.set_reasoning_effort(rust_agent_client::ReasoningEffort::High);

    // List available DeepSeek models (provider-specific API)
    match ds_client.list_models().await {
        Ok(models) => {
            tracing::info!("DeepSeek models: {}",
                models.iter().take(5).map(|m| m.id.as_str()).collect::<Vec<_>>().join(", "));
        }
        Err(e) => tracing::warn!("list_models failed (check API key): {}", e),
    }

    // ---- Full conversation flow with DeepSeek ----
    // 1. Create tools
    let mut tools = ToolRegistry::new();
    tools.register(Echo);
    tools.register(Add);

    // 2. Create agent (IAgent) with DeepSeek
    let agent = ChatClientAgent::new("assistant", Arc::new(ds_client))
        .with_instructions("You are a helpful AI assistant.")
        .with_tools(tools)
        .with_description("A general-purpose assistant agent");

    // 3. Create session (ISession)
    let session = Arc::new(AgentSession::new());
    session.add_message(ChatMessage::user("Hello, world!")).await?;

    // 4. Run agent — streaming output
    let messages = session.get_messages().await?;
    let stream = agent.run(messages).await?;

    print!("Agent [{}]: ", agent.id());
    let response = collect_agent_response(stream).await?;
    println!("{}", response.text);

    // 5. Store assistant response in session for conversation continuity
    session.add_message(ChatMessage::assistant(&response.text)).await?;

    // 6. Workflow example (IWorkflow)
    let mut workflow = GraphFlow::new();
    workflow.add_agent(Arc::new(agent));
    workflow.set_entry(AgentId::new("assistant"));

    let wf_stream = workflow.run(vec![ChatMessage::user("Hello from workflow!")]).await?;
    print!("Workflow: ");
    let wf_response = collect_agent_response(wf_stream).await?;
    println!("{}", wf_response.text);

    Ok(())
}
