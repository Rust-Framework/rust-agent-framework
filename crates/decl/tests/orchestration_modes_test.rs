#![cfg(feature = "yaml")]

//! 声明式编排模式构建测试

use std::path::PathBuf;

use rust_agent_core::AgentId;
use rust_agent_decl::DeclAgentBuilder;

fn coding_agent_yaml_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../cli/coding-agent.yaml")
}

fn minimal_sub_agent(name: &str) -> String {
    format!(
        r#"
  - kind: prompt
    name: {name}
    model:
      id: gpt-4o
      connection:
        kind: key
        api_key: sk-test
    instructions: "test agent {name}"
    tools: []
"#
    )
}

fn base_yaml(orchestration: &str, subs: impl AsRef<str>) -> String {
    let subs = subs.as_ref();
    format!(
        r#"
kind: prompt
name: orch-test
model:
  id: gpt-4o
  connection:
    kind: key
    api_key: sk-test
instructions: orchestrator
metadata:
  orchestration: {orchestration}
subAgents:
{subs}
"#
    )
}

#[tokio::test]
async fn build_pipeline_coding_agent() {
    let agent = DeclAgentBuilder::new()
        .from_yaml_file(coding_agent_yaml_path())
        .build()
        .await
        .expect("pipeline coding-agent builds");

    assert_eq!(agent.id().to_string(), "coding-agent");
    assert!(agent.get_subagent(&AgentId::new("coder-alpha")).is_some());
}

#[tokio::test]
async fn build_sequential_orchestration() {
    let yaml = base_yaml(
        "sequential",
        minimal_sub_agent("a") + &minimal_sub_agent("b"),
    );
    DeclAgentBuilder::new()
        .from_yaml_str(&yaml)
        .build()
        .await
        .expect("sequential builds");
}

#[tokio::test]
async fn build_concurrent_orchestration() {
    let yaml = base_yaml(
        "concurrent",
        minimal_sub_agent("a") + &minimal_sub_agent("b"),
    );
    DeclAgentBuilder::new()
        .from_yaml_str(&yaml)
        .build()
        .await
        .expect("concurrent builds");
}

#[tokio::test]
async fn build_magentic_shorthand() {
    let yaml = base_yaml("magentic", &minimal_sub_agent("worker"));
    DeclAgentBuilder::new()
        .from_yaml_str(&yaml)
        .build()
        .await
        .expect("magentic shorthand builds");
}

#[tokio::test]
async fn build_handoff_with_triage_subagent() {
    let yaml = format!(
        r#"
kind: prompt
name: handoff-test
model:
  id: gpt-4o
  connection:
    kind: key
    api_key: sk-test
instructions: root
metadata:
  orchestration:
    mode: handoff
    triage: router
subAgents:
  - kind: prompt
    name: router
    description: routes to billing expert
    model:
      id: gpt-4o
      connection:
        kind: key
        api_key: sk-test
    instructions: triage
  - kind: prompt
    name: billing
    description: billing expert
    model:
      id: gpt-4o
      connection:
        kind: key
        api_key: sk-test
    instructions: billing
"#
    );
    DeclAgentBuilder::new()
        .from_yaml_str(&yaml)
        .build()
        .await
        .expect("handoff builds");
}

#[tokio::test]
async fn build_vote_orchestration() {
    let yaml = format!(
        r#"
kind: prompt
name: vote-test
model:
  id: gpt-4o
  connection:
    kind: key
    api_key: sk-test
instructions: vote
metadata:
  orchestration:
    mode: vote
    aggregator: majority
subAgents:
{}
{}
"#,
        minimal_sub_agent("voter-a"),
        minimal_sub_agent("voter-b")
    );
    DeclAgentBuilder::new()
        .from_yaml_str(&yaml)
        .build()
        .await
        .expect("vote builds");
}

#[tokio::test]
async fn build_group_chat_with_selector() {
    let yaml = format!(
        r#"
kind: prompt
name: group-chat-test
model:
  id: gpt-4o
  connection:
    kind: key
    api_key: sk-test
instructions: coordinator
metadata:
  orchestration:
    mode: groupChat
    coordinator: coord
    maxRounds: 3
    selector: roundRobin
subAgents:
  - kind: prompt
    name: coord
    model:
      id: gpt-4o
      connection:
        kind: key
        api_key: sk-test
    instructions: coordinate
  - kind: prompt
    name: alice
    model:
      id: gpt-4o
      connection:
        kind: key
        api_key: sk-test
    instructions: alice
  - kind: prompt
    name: bob
    model:
      id: gpt-4o
      connection:
        kind: key
        api_key: sk-test
    instructions: bob
"#
    );
    let agent = DeclAgentBuilder::new()
        .from_yaml_str(&yaml)
        .build()
        .await
        .expect("groupChat with selector builds");
    assert_eq!(agent.id().to_string(), "group-chat-test");
    assert!(agent.get_subagent(&AgentId::new("alice")).is_some());
}
