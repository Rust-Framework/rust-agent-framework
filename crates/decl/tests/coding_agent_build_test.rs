//! End-to-end build verification for crates/cli/coding-agent.yaml

#![cfg(feature = "yaml")]

use std::path::PathBuf;

use rust_agent_core::AgentId;
use rust_agent_decl::DeclAgentBuilder;

fn coding_agent_yaml_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../cli/coding-agent.yaml")
}

#[tokio::test]
async fn coding_agent_yaml_parses() {
    let yaml = std::fs::read_to_string(coding_agent_yaml_path()).expect("read coding-agent.yaml");
    let doc = rust_agent_decl::AgentDocument::from_yaml_str(&yaml).expect("parse yaml");
    let def = doc.inner_definition();
    assert_eq!(def.name, "coding-agent");

    let prompt = match &def.kind_data {
        rust_agent_decl::AgentKindData::Prompt(data) => data,
        _ => panic!("expected prompt agent"),
    };
    assert_eq!(prompt.sub_agents.len(), 6);
    let orch = def.metadata.get("orchestration").expect("orchestration");
    let mode = orch.get("mode").and_then(|v| v.as_str()).unwrap_or("");
    assert_eq!(mode, "pipeline");
    let phases = orch.get("phases").and_then(|v| v.as_array()).expect("phases");
    assert_eq!(phases.len(), 3);
}

#[tokio::test]
async fn coding_agent_yaml_builds_with_sub_agents() {
    let agent = DeclAgentBuilder::new()
        .from_yaml_file(coding_agent_yaml_path())
        .build()
        .await
        .expect("build coding-agent");

    for name in [
        "planner",
        "explorer",
        "coder-alpha",
        "coder-beta",
        "tester",
        "reviewer",
    ] {
        assert!(
            agent.get_subagent(&AgentId::new(name)).is_some(),
            "missing sub-agent: {name}"
        );
    }
}

#[tokio::test]
async fn coding_agent_orchestrator_tools_validate() {
    let report = DeclAgentBuilder::new()
        .from_yaml_file(coding_agent_yaml_path())
        .validate()
        .await
        .expect("validate coding-agent");

    assert!(
        report.errors.is_empty(),
        "validation errors: {:?}",
        report.errors
    );
    assert!(
        report.resolved_tools.iter().any(|t| t.contains("web_search")),
        "orchestrator should resolve web tools: {:?}",
        report.resolved_tools
    );
    assert!(
        report
            .resolved_providers
            .iter()
            .any(|p| p.starts_with("memory(")),
        "memory provider: {:?}",
        report.resolved_providers
    );
    assert!(
        report
            .resolved_providers
            .iter()
            .any(|p| p.starts_with("workspace(")),
        "workspace provider: {:?}",
        report.resolved_providers
    );
}

#[tokio::test]
async fn coding_agent_sub_agent_tools_resolve() {
    let yaml = std::fs::read_to_string(coding_agent_yaml_path()).expect("read yaml");
    let doc = rust_agent_decl::AgentDocument::from_yaml_str(&yaml).unwrap();
    let def = doc.inner_definition();
    let prompt = match &def.kind_data {
        rust_agent_decl::AgentKindData::Prompt(data) => data,
        _ => panic!("expected prompt"),
    };

    let resolver = rust_agent_decl::ToolResolver::new();
    for sub in &prompt.sub_agents {
        let sub_prompt = match &sub.kind_data {
            rust_agent_decl::AgentKindData::Prompt(data) => data,
            _ => panic!("sub-agent must be prompt"),
        };
        let tools = resolver.resolve_all(&sub_prompt.tools).await.unwrap_or_else(|e| {
            panic!("failed to resolve tools for {}: {e}", sub.name);
        });
        assert!(
            !tools.is_empty(),
            "sub-agent '{}' should have at least one tool",
            sub.name
        );
    }
}
