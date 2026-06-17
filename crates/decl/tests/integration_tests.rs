use std::collections::HashMap;

use rust_agent_decl::{
    AgentDecl, DeclError,
    DefaultAgentResolver, EdgeDecl, ModelConfig, NodeDecl,
    ToolRef, WorkflowDecl,
    resolver::AgentResolver,
};

// ── AgentDecl Parsing Tests ──

#[test]
fn test_agent_decl_from_json_str() {
    let json = r#"{
        "id": "test-agent",
        "description": "A test agent",
        "instructions": "You are a helpful assistant.",
        "model": {
            "provider": "openai",
            "model": "gpt-4o",
            "api_key": "$OPENAI_API_KEY"
        },
        "tools": [
            { "type": "builtin", "name": "read_file" },
            { "type": "builtin", "name": "web_search" }
        ],
        "max_tool_rounds": 5
    }"#;

    let decl = AgentDecl::from_json_str(json).unwrap();
    assert_eq!(decl.id, "test-agent");
    assert_eq!(decl.description, "A test agent");
    assert_eq!(decl.instructions, "You are a helpful assistant.");
    assert_eq!(decl.model.provider, "openai");
    assert_eq!(decl.model.model, "gpt-4o");
    assert_eq!(decl.max_tool_rounds, 5);
    assert_eq!(decl.tools.len(), 2);
}

#[test]
fn test_agent_decl_minimal_json() {
    let json = r#"{
        "id": "minimal",
        "model": { "provider": "deepseek", "model": "deepseek-chat", "api_key": "sk-xxx" }
    }"#;

    let decl = AgentDecl::from_json_str(json).unwrap();
    assert_eq!(decl.id, "minimal");
    assert_eq!(decl.max_tool_rounds, 10); // default
    assert!(decl.instructions.is_empty());
    assert!(decl.tools.is_empty());
    assert!(decl.sub_agents.is_empty());
}

#[test]
fn test_agent_decl_with_sub_agents() {
    let json = r#"{
        "id": "orchestrator",
        "model": { "provider": "openai", "model": "gpt-4o", "api_key": "sk-xxx" },
        "sub_agents": [
            {
                "id": "child-1",
                "model": { "provider": "deepseek", "model": "deepseek-chat", "api_key": "sk-yyy" },
                "instructions": "Child agent"
            }
        ]
    }"#;

    let decl = AgentDecl::from_json_str(json).unwrap();
    assert_eq!(decl.sub_agents.len(), 1);
    assert_eq!(decl.sub_agents[0].id, "child-1");
    assert_eq!(decl.sub_agents[0].instructions, "Child agent");
}

#[test]
fn test_agent_decl_invalid_json() {
    let result = AgentDecl::from_json_str("not valid json");
    assert!(result.is_err());
}

#[test]
fn test_model_config_api_key_env_var() {
    let model = ModelConfig {
        provider: "openai".into(),
        model: "gpt-4o".into(),
        api_key: Some("$MY_API_KEY".into()),
        base_url: None,
        temperature: None,
        max_tokens: None,
        extra_headers: HashMap::new(),
        extra: HashMap::new(),
    };

    // Without env var set, should error
    std::env::remove_var("MY_API_KEY");
    let result = model.resolve_api_key();
    assert!(result.is_err());

    // With env var set, should succeed
    std::env::set_var("MY_API_KEY", "test-key-123");
    let key = model.resolve_api_key().unwrap();
    assert_eq!(key, "test-key-123");
    std::env::remove_var("MY_API_KEY");
}

#[test]
fn test_model_config_direct_api_key() {
    let model = ModelConfig {
        provider: "openai".into(),
        model: "gpt-4o".into(),
        api_key: Some("direct-key-456".into()),
        base_url: None,
        temperature: None,
        max_tokens: None,
        extra_headers: HashMap::new(),
        extra: HashMap::new(),
    };

    let key = model.resolve_api_key().unwrap();
    assert_eq!(key, "direct-key-456");
}

#[test]
fn test_agent_decl_with_compression() {
    let json = r#"{
        "id": "compressed-agent",
        "model": { "provider": "openai", "model": "gpt-4o", "api_key": "sk-xxx" },
        "compression": { "type": "sliding_window", "window_size": 100 },
        "token_counter": { "type": "estimate" }
    }"#;

    let decl = AgentDecl::from_json_str(json).unwrap();
    assert!(decl.compression.is_some());
    assert!(decl.token_counter.is_some());
}

#[test]
fn test_agent_decl_tool_ref_rhai() {
    let json = r#"{
        "id": "rhai-agent",
        "model": { "provider": "openai", "model": "gpt-4o", "api_key": "sk-xxx" },
        "tools": [
            {
                "type": "rhai",
                "name": "my_tool",
                "description": "A Rhai script tool",
                "script_path": "./tools/my_tool.rhai",
                "parameters": { "type": "object", "properties": {} }
            }
        ]
    }"#;

    let decl = AgentDecl::from_json_str(json).unwrap();
    assert_eq!(decl.tools.len(), 1);
    match &decl.tools[0] {
        ToolRef::Rhai { name, script_path, .. } => {
            assert_eq!(name, "my_tool");
            assert_eq!(script_path, "./tools/my_tool.rhai");
        }
        _ => panic!("Expected Rhai tool ref"),
    }
}

#[test]
fn test_agent_decl_tool_ref_custom() {
    let json = r#"{
        "id": "custom-tool-agent",
        "model": { "provider": "openai", "model": "gpt-4o", "api_key": "sk-xxx" },
        "tools": [
            {
                "type": "custom",
                "name": "database_query"
            }
        ]
    }"#;

    let decl = AgentDecl::from_json_str(json).unwrap();
    assert_eq!(decl.tools.len(), 1);
    match &decl.tools[0] {
        ToolRef::Custom { name, .. } => {
            assert_eq!(name, "database_query");
        }
        _ => panic!("Expected Custom tool ref"),
    }
}

// ── WorkflowDecl Parsing Tests ──

#[test]
fn test_workflow_decl_from_json_str() {
    let json = r#"{
        "name": "test-workflow",
        "nodes": [
            { "type": "agent", "id": "researcher", "agent_ref": "research-agent" },
            { "type": "agent", "id": "writer", "agent_ref": "writer-agent", "is_output": true }
        ],
        "edges": [
            { "type": "direct", "source": "researcher", "target": "writer" }
        ],
        "start_node_id": "researcher",
        "output_node_ids": ["writer"]
    }"#;

    let decl = WorkflowDecl::from_json_str(json).unwrap();
    assert_eq!(decl.name, "test-workflow");
    assert_eq!(decl.nodes.len(), 2);
    assert_eq!(decl.edges.len(), 1);
    assert_eq!(decl.start_node_id, "researcher");
    assert_eq!(decl.output_node_ids.len(), 1);
}

#[test]
fn test_workflow_decl_with_fan_edges() {
    let json = r#"{
        "name": "fan-workflow",
        "nodes": [
            { "type": "agent", "id": "start", "agent_ref": "agent-a" },
            { "type": "agent", "id": "worker-1", "agent_ref": "agent-b" },
            { "type": "agent", "id": "worker-2", "agent_ref": "agent-c" },
            { "type": "agent", "id": "end", "agent_ref": "agent-d" }
        ],
        "edges": [
            { "type": "fan_out", "source": "start", "targets": ["worker-1", "worker-2"] },
            { "type": "fan_in", "sources": ["worker-1", "worker-2"], "target": "end" }
        ],
        "start_node_id": "start"
    }"#;

    let decl = WorkflowDecl::from_json_str(json).unwrap();
    assert_eq!(decl.edges.len(), 2);

    match &decl.edges[0] {
        EdgeDecl::FanOut { source, targets } => {
            assert_eq!(source, "start");
            assert_eq!(targets.len(), 2);
        }
        _ => panic!("Expected FanOut edge"),
    }

    match &decl.edges[1] {
        EdgeDecl::FanIn { sources, target } => {
            assert_eq!(sources.len(), 2);
            assert_eq!(target, "end");
        }
        _ => panic!("Expected FanIn edge"),
    }
}

#[test]
fn test_workflow_decl_inline_agent_node() {
    let json = r#"{
        "name": "inline-workflow",
        "nodes": [
            {
                "type": "agent",
                "id": "inline-agent",
                "agent_ref": "",
                "agent": {
                    "id": "inline-1",
                    "model": { "provider": "openai", "model": "gpt-4o", "api_key": "sk-xxx" }
                }
            }
        ],
        "start_node_id": "inline-agent"
    }"#;

    let decl = WorkflowDecl::from_json_str(json).unwrap();
    match &decl.nodes[0] {
        NodeDecl::Agent { id, agent, .. } => {
            assert_eq!(id, "inline-agent");
            assert!(agent.is_some());
        }
        _ => panic!("Expected Agent node"),
    }
}

#[test]
fn test_workflow_decl_rhai_node() {
    let json = r#"{
        "name": "rhai-workflow",
        "nodes": [
            { "type": "rhai", "id": "transform", "script_path": "./scripts/transform.rhai" }
        ],
        "start_node_id": "transform"
    }"#;

    let decl = WorkflowDecl::from_json_str(json).unwrap();
    match &decl.nodes[0] {
        NodeDecl::Rhai { id, script_path, .. } => {
            assert_eq!(id, "transform");
            assert_eq!(script_path, "./scripts/transform.rhai");
        }
        _ => panic!("Expected Rhai node"),
    }
}

// ── DefaultAgentResolver Tests ──

#[test]
fn test_resolve_builtin_tool_read_file() {
    let tool = DefaultAgentResolver::resolve_builtin_tool("read_file", &std::collections::HashMap::new()).unwrap();
    assert_eq!(tool.name(), "read_file");
    assert!(!tool.description().is_empty());
}

#[test]
fn test_resolve_builtin_tool_web_search() {
    let tool = DefaultAgentResolver::resolve_builtin_tool("web_search", &std::collections::HashMap::new()).unwrap();
    assert_eq!(tool.name(), "web_search");
}

#[test]
fn test_resolve_builtin_tool_all() {
    let names = [
        "read_file", "write_file", "edit_file", "list_files",
        "inspect_file", "make_directory", "remove_path", "move_file",
        "find_files", "search_file", "run_command", "web_search", "web_fetch",
    ];
    for name in &names {
        let tool = DefaultAgentResolver::resolve_builtin_tool(name, &std::collections::HashMap::new()).unwrap();
        assert_eq!(tool.name(), *name, "Tool name mismatch for '{}'", name);
    }
}

#[test]
fn test_resolve_builtin_tool_unknown() {
    let result = DefaultAgentResolver::resolve_builtin_tool("nonexistent_tool", &std::collections::HashMap::new());
    assert!(result.is_err());
}

// ── Serialization Round-trip Tests ──

#[test]
fn test_agent_decl_roundtrip_json() {
    let json = r#"{"id":"rt-agent","model":{"provider":"openai","model":"gpt-4o","api_key":"sk-xxx"}}"#;
    let decl = AgentDecl::from_json_str(json).unwrap();
    let out = decl.to_json_string().unwrap();
    let decl2 = AgentDecl::from_json_str(&out).unwrap();
    assert_eq!(decl.id, decl2.id);
    assert_eq!(decl.model.model, decl2.model.model);
}

#[test]
fn test_agent_decl_to_json_pretty() {
    let json = r#"{"id":"pretty","model":{"provider":"openai","model":"gpt-4o","api_key":"sk-xxx"}}"#;
    let decl = AgentDecl::from_json_str(json).unwrap();
    let out = decl.to_json_pretty().unwrap();
    assert!(out.contains('\n'));
    assert!(out.contains("pretty"));
}

#[test]
fn test_workflow_decl_roundtrip_json() {
    let json = r#"{"name":"rt-wf","nodes":[{"type":"agent","id":"n1","agent_ref":"a1"}],"start_node_id":"n1"}"#;
    let decl = WorkflowDecl::from_json_str(json).unwrap();
    let out = decl.to_json_string().unwrap();
    let decl2 = WorkflowDecl::from_json_str(&out).unwrap();
    assert_eq!(decl.name, decl2.name);
    assert_eq!(decl.start_node_id, decl2.start_node_id);
}

#[test]
fn test_workflow_decl_roundtrip_complex() {
    let json = r#"{
        "name": "complex-wf",
        "nodes": [
            { "type": "agent", "id": "n1", "agent_ref": "a1" },
            { "type": "rhai", "id": "n2", "script_path": "./t.rhai" }
        ],
        "edges": [
            { "type": "direct", "source": "n1", "target": "n2" }
        ],
        "start_node_id": "n1",
        "output_node_ids": ["n2"]
    }"#;
    let decl = WorkflowDecl::from_json_str(json).unwrap();
    let out = decl.to_json_string().unwrap();
    let decl2 = WorkflowDecl::from_json_str(&out).unwrap();
    assert_eq!(decl.name, decl2.name);
    assert_eq!(decl.nodes.len(), decl2.nodes.len());
    assert_eq!(decl.edges.len(), decl2.edges.len());
    assert_eq!(decl.output_node_ids, decl2.output_node_ids);
}

// ── DefaultAgentResolver Custom Tool Factory Tests ──

#[tokio::test]
async fn test_custom_tool_factory_registration() {
    let mut resolver = DefaultAgentResolver::new();
    resolver.register_tool_factory("my_custom", |_config| {
        Err(DeclError::Unsupported("not implemented in test".into()))
    });

    let result = resolver.resolve_tool(&ToolRef::Custom {
        name: "my_custom".into(),
        config: HashMap::new(),
    }).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_custom_tool_factory_not_registered() {
    let resolver = DefaultAgentResolver::new();
    let result = resolver.resolve_tool(&ToolRef::Custom {
        name: "unregistered".into(),
        config: HashMap::new(),
    }).await;
    assert!(result.is_err());
}
