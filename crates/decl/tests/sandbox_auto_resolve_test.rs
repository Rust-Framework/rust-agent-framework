#![cfg(all(feature = "yaml", feature = "sandbox"))]

//! 声明式 sandbox 无需 with_tool 工厂

use rust_agent_decl::DeclAgentBuilder;

#[tokio::test]
async fn code_interpreter_auto_resolves_with_sandbox_feature() {
    let yaml = r#"
kind: prompt
name: auto-sandbox-agent
model:
  id: gpt-4o
  connection:
    kind: key
    api_key: sk-test
instructions: test
tools:
  - kind: code
    name: code_interpreter
    config:
      backend: process
"#;

    let agent = DeclAgentBuilder::new()
        .from_yaml_str(yaml)
        .build()
        .await
        .expect("auto sandbox agent builds");

    assert_eq!(agent.id().to_string(), "auto-sandbox-agent");
}
