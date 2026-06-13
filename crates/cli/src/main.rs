use std::sync::Arc;

use futures_util::StreamExt;
use rust_agent_client::{ChatClientConfig, OpenAIChatClient};
use rust_agent_core::{
    tool, AgentId, AgentSession, ChatMessage, IAgent, ISession, IWorkflow,
    ToolRegistry,
};
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

    // 1. Create chat client (IChatClient)
    let config = ChatClientConfig::default();
    let chat_client = Arc::new(OpenAIChatClient::new(config));

    // 2. Create tools (ITool) — just instantiate the macro-generated structs
    let mut tools = ToolRegistry::new();
    tools.register(Echo);
    tools.register(Add);

    // 3. Create agent (IAgent)
    let agent = ChatClientAgent::new("assistant", chat_client)
        .with_instructions("You are a helpful AI assistant.")
        .with_tools(tools)
        .with_description("A general-purpose assistant agent");

    // 4. Create session (ISession)
    let session = AgentSession::new();
    session.add_message(ChatMessage::user("Hello, world!")).await?;

    // 5. Run agent
    let mut stream = agent.run(session.get_messages().await?).await?;
    print!("Agent [{}]: ", agent.id());
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if let Some(delta) = chunk.text_delta {
            print!("{}", delta);
        }
    }
    println!();

    // 6. Workflow example (IWorkflow)
    let mut workflow = GraphFlow::new();
    workflow.add_agent(Arc::new(agent));
    workflow.set_entry(AgentId::new("assistant"));

    let mut wf_stream = workflow.run(vec![ChatMessage::user("Hello from workflow!")]).await?;
    print!("Workflow: ");
    while let Some(chunk) = wf_stream.next().await {
        let chunk = chunk?;
        if let Some(delta) = chunk.text_delta {
            print!("{}", delta);
        }
    }
    println!();

    Ok(())
}
