#![cfg(feature = "yaml")]

//! Reference 连接解析测试

use rust_agent_decl::{Connection, ConnectionDetails, ConnectionKind, DeclAgentBuilder};

#[tokio::test]
async fn reference_connection_resolves_via_builder_registry() {
    let yaml = r#"
kind: prompt
name: ref-agent
model:
  id: gpt-4o
  connection:
    kind: reference
    name: shared-openai
instructions: test
"#;

    let agent = DeclAgentBuilder::new()
        .from_yaml_str(yaml)
        .with_connection(
            "shared-openai",
            Connection {
                kind: ConnectionKind::ApiKey,
                authentication_mode: rust_agent_decl::AuthenticationMode::System,
                usage_description: None,
                details: ConnectionDetails {
                    api_key: Some("sk-test".into()),
                    ..Default::default()
                },
            },
        )
        .build()
        .await
        .expect("reference connection agent builds");

    assert_eq!(agent.id().to_string(), "ref-agent");
}
