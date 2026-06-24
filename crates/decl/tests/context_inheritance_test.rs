//! 子 Agent 继承父 Agent contexts 的单元测试

use rust_agent_decl::context_inheritance::{inherit_parent_contexts, parent_has_workspace};
use rust_agent_decl::context_provider_config::ContextProviderDecl;
use rust_agent_decl::definition::{AgentDefinition, AgentKindData};
use rust_agent_decl::prompt_agent::PromptAgentData;
use std::collections::HashMap;

fn minimal_prompt_data() -> PromptAgentData {
    serde_json::from_value(serde_json::json!({
        "model": {
            "id": "gpt-4o",
            "connection": { "kind": "key", "api_key": "sk-test" }
        },
        "instructions": "test"
    }))
    .unwrap()
}

fn minimal_sub_def(name: &str) -> AgentDefinition {
    AgentDefinition {
        name: name.to_string(),
        display_name: None,
        description: String::new(),
        metadata: HashMap::new(),
        input_schema: None,
        output_schema: None,
        kind_data: AgentKindData::Prompt(minimal_prompt_data()),
    }
}

#[test]
fn inherit_workspace_from_parent() {
    let mut parent = minimal_prompt_data();
    parent.contexts.push(ContextProviderDecl::Workspace {
        name: "default".to_string(),
        config: HashMap::from([
            ("root".to_string(), serde_json::json!(".")),
            ("policy".to_string(), serde_json::json!("read")),
        ]),
    });

    let mut sub = minimal_sub_def("child");
    inherit_parent_contexts(&mut sub, &parent);

    let sub_data = match &sub.kind_data {
        AgentKindData::Prompt(d) => d,
        _ => panic!("expected prompt"),
    };
    assert_eq!(sub_data.contexts.len(), 1);
    assert!(matches!(
        &sub_data.contexts[0],
        ContextProviderDecl::Workspace { name, .. } if name == "default"
    ));
}

#[test]
fn inherit_does_not_duplicate_same_named_context() {
    let mut parent = minimal_prompt_data();
    parent.contexts.push(ContextProviderDecl::Workspace {
        name: "default".to_string(),
        config: HashMap::from([("root".to_string(), serde_json::json!("."))]),
    });
    parent.contexts.push(ContextProviderDecl::Bundle {
        name: "knowledge-bundle".to_string(),
        config: HashMap::new(),
    });

    let mut sub = minimal_sub_def("child");
    if let AgentKindData::Prompt(ref mut data) = sub.kind_data {
        data.contexts.push(ContextProviderDecl::Workspace {
            name: "default".to_string(),
            config: HashMap::from([("root".to_string(), serde_json::json!("/custom"))]),
        });
    }

    inherit_parent_contexts(&mut sub, &parent);

    let sub_data = match &sub.kind_data {
        AgentKindData::Prompt(d) => d,
        _ => panic!("expected prompt"),
    };
    assert_eq!(sub_data.contexts.len(), 2);
    assert!(sub_data.contexts.iter().any(|c| {
        matches!(c, ContextProviderDecl::Bundle { name, .. } if name == "knowledge-bundle")
    }));
    if let ContextProviderDecl::Workspace { config, .. } = &sub_data.contexts[0] {
        assert_eq!(config.get("root").and_then(|v| v.as_str()), Some("/custom"));
    } else {
        panic!("child workspace config should be preserved");
    }
}

#[test]
fn parent_has_workspace_detects_workspace_context() {
    let mut parent = minimal_prompt_data();
    assert!(!parent_has_workspace(&parent));

    parent.contexts.push(ContextProviderDecl::Skills {
        name: "antd".to_string(),
        config: HashMap::new(),
    });
    assert!(!parent_has_workspace(&parent));

    parent.contexts.push(ContextProviderDecl::Workspace {
        name: "default".to_string(),
        config: HashMap::new(),
    });
    assert!(parent_has_workspace(&parent));
}
