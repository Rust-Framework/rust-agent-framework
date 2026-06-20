#![cfg(all(feature = "yaml", feature = "wiki"))]

//! Wiki 上下文声明式构建测试

use rust_agent_decl::DeclAgentBuilder;

#[tokio::test]
async fn wiki_context_provider_builds_from_yaml() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("page.md"),
        "---\ntitle: Test Page\n---\n\nWiki page about agents.",
    )
    .unwrap();

    let yaml = format!(
        r#"
kind: prompt
name: wiki-agent
model:
  id: gpt-4o
  connection:
    kind: key
    api_key: sk-test
instructions: test
contexts:
  - kind: wiki
    name: docs
    config:
      source: {}
"#,
        dir.path().display()
    );

    let agent = DeclAgentBuilder::new()
        .from_yaml_str(&yaml)
        .build()
        .await
        .expect("wiki agent builds");

    assert_eq!(agent.id().to_string(), "wiki-agent");
}
