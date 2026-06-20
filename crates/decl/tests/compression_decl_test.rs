#![cfg(feature = "yaml")]

//! 声明式 compression / token_counter 集成测试

use rust_agent_decl::DeclAgentBuilder;

#[tokio::test]
async fn sliding_window_compression_wires_to_agent_builder() {
    let yaml = r#"
kind: prompt
name: compress-agent
model:
  id: gpt-4o
  connection:
    kind: key
    api_key: sk-test
instructions: test
compression:
  kind: sliding_window
  windowSize: 20
tokenCounter:
  kind: estimate
"#;

    let agent = DeclAgentBuilder::new()
        .from_yaml_str(yaml)
        .build()
        .await
        .expect("agent with compression builds");

    assert_eq!(agent.id().to_string(), "compress-agent");
}
