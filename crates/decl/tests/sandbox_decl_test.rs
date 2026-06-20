#![cfg(all(feature = "yaml", feature = "sandbox"))]

//! 声明式 sandbox backend 配置测试

use rust_agent_decl::DeclAgentBuilder;

#[tokio::test]
async fn code_interpreter_builds_with_process_backend() {
    let yaml = r#"
kind: prompt
name: code-agent
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
      default_language: python
"#;

    let agent = DeclAgentBuilder::new()
        .from_yaml_str(yaml)
        .build()
        .await
        .expect("code agent builds");

    assert_eq!(agent.id().to_string(), "code-agent");
}
