//! Integration tests for the declarative crate.
//!
//! Tests parsing, serialization, and basic resolution of MAF-aligned types.

use rust_agent_decl::{
    AgentDocument, AgentDefinition, AgentKindData,
    ToolDecl, ToolResolver,
    Model, ModelOptions, Connection,
    PropertySchema, Property, PropertyType,
    Template,
};

// ── Parsing Tests ──

#[test]
fn parse_minimal_prompt_agent_json() {
    let json = r#"{
        "kind": "prompt",
        "name": "minimal",
        "model": {
            "id": "gpt-4o",
            "connection": {
                "kind": "key",
                "api_key": "sk-test"
            }
        }
    }"#;

    let doc = AgentDocument::from_json_str(json).unwrap();
    let def = doc.inner_definition();
    assert_eq!(def.name, "minimal");
    assert!(def.is_prompt());
}

#[test]
fn parse_prompt_agent_with_all_fields_json() {
    let json = r#"{
        "kind": "prompt",
        "name": "full-agent",
        "displayName": "Full Agent",
        "description": "A complete agent definition",
        "metadata": {
            "authors": ["test"],
            "tags": ["example"]
        },
        "inputSchema": {
            "properties": [
                {
                    "name": "question",
                    "kind": "string",
                    "required": true,
                    "description": "The user question"
                }
            ]
        },
        "model": {
            "id": "gpt-4o",
            "provider": "openai",
            "connection": {
                "kind": "key",
                "api_key": "sk-test"
            },
            "options": {
                "kind": "standard",
                "temperature": 0.7,
                "maxOutputTokens": 2048
            }
        },
        "tools": [
            { "kind": "web_search" },
            {
                "kind": "function",
                "name": "read_file",
                "description": "Read file contents"
            }
        ],
        "template": {
            "format": "mustache",
            "parser": "prompty"
        },
        "instructions": "You are a helpful assistant.",
        "maxToolRounds": 5
    }"#;

    let doc = AgentDocument::from_json_str(json).unwrap();
    let def = doc.inner_definition();
    assert_eq!(def.name, "full-agent");
    assert_eq!(def.display_name, Some("Full Agent".to_string()));
    assert!(def.input_schema.is_some());
    assert_eq!(def.metadata.get("authors").unwrap().as_array().unwrap().len(), 1);

    if let AgentKindData::Prompt(data) = &def.kind_data {
        assert_eq!(data.model.id, "gpt-4o");
        assert_eq!(data.tools.len(), 2);
        assert!(data.template.is_some());
        assert_eq!(data.instructions, "You are a helpful assistant.");
        assert_eq!(data.max_tool_rounds, 5);
    }
}

#[test]
fn parse_workflow_definition() {
    let json = r#"{
        "kind": "workflow",
        "name": "test-wf",
        "trigger": {
            "kind": "OnConversationStart",
            "id": "wf_1",
            "actions": [
                {
                    "kind": "SetVariable",
                    "variable": "Local.greeting",
                    "value": "Hello"
                },
                {
                    "kind": "SendActivity",
                    "activity": { "text": "Done" }
                }
            ]
        }
    }"#;

    let doc = AgentDocument::from_json_str(json).unwrap();
    let def = doc.inner_definition();
    assert!(def.is_workflow());

    if let AgentKindData::Workflow(data) = &def.kind_data {
        assert_eq!(data.trigger.id, "wf_1");
        assert_eq!(data.trigger.actions.len(), 2);
    }
}

#[test]
fn parse_container_definition() {
    let json = r#"{
        "kind": "hosted",
        "name": "my-container",
        "protocols": [
            { "protocol": "responses", "version": "v0.1.1" }
        ],
        "image": "registry.io/agent:latest",
        "resources": {
            "cpu": "1",
            "memory": "2Gi"
        }
    }"#;

    let doc = AgentDocument::from_json_str(json).unwrap();
    let def = doc.inner_definition();
    assert!(def.is_container());

    if let AgentKindData::Container(data) = &def.kind_data {
        assert_eq!(data.protocols.len(), 1);
        assert_eq!(data.image.as_deref(), Some("registry.io/agent:latest"));
        assert_eq!(data.resources.cpu, "1");
    }
}

// ── Serialization Tests ──

#[test]
fn agent_definition_roundtrip_json() {
    let json = r#"{"kind":"prompt","name":"rt","model":{"id":"gpt-4o","connection":{"kind":"key","api_key":"sk"}}}"#;
    let doc = AgentDocument::from_json_str(json).unwrap();
    let serialized = doc.to_json_string().unwrap();
    let doc2 = AgentDocument::from_json_str(&serialized).unwrap();
    assert_eq!(doc.inner_definition().name, doc2.inner_definition().name);
}

#[test]
fn agent_definition_pretty_json() {
    let json = r#"{"kind":"prompt","name":"pretty","model":{"id":"gpt-4o","connection":{"kind":"key","api_key":"sk"}}}"#;
    let doc = AgentDocument::from_json_str(json).unwrap();
    let pretty = doc.to_json_pretty().unwrap();
    assert!(pretty.contains('\n'));
    assert!(pretty.contains("pretty"));
}

// ── Model Tests ──

#[test]
fn model_builder() {
    let model = Model::new("gpt-4o")
        .with_provider("openai")
        .with_connection(Connection::api_key("sk-test"));

    assert_eq!(model.id, "gpt-4o");
    assert_eq!(model.provider.as_deref(), Some("openai"));
}

#[test]
fn model_options_builder() {
    let opts = ModelOptions::new("standard")
        .with_temperature(0.7)
        .with_max_output_tokens(4096);

    assert_eq!(opts.kind, "standard");
    assert_eq!(opts.temperature, Some(0.7));
    assert_eq!(opts.max_output_tokens, Some(4096));
}

// ── Tool Tests ──

#[test]
fn tool_decl_kind_str() {
    assert_eq!(ToolDecl::WebSearch.kind_str(), "web_search");
    assert_eq!(ToolDecl::CodeInterpreter.kind_str(), "code_interpreter");

    let func = ToolDecl::function("my_fn", "My function");
    assert_eq!(func.kind_str(), "function");
    assert_eq!(func.name(), Some("my_fn"));
}

#[test]
fn tool_resolver_web_search() {
    let resolver = ToolResolver::new();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let tool = rt.block_on(resolver.resolve(&ToolDecl::WebSearch)).unwrap();
    assert_eq!(tool.name(), "web_search");
}

#[test]
fn tool_resolver_function_builtin() {
    let resolver = ToolResolver::new();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let tool_decl = ToolDecl::function("read_file", "Read file");
    let tool = rt.block_on(resolver.resolve(&tool_decl)).unwrap();
    assert_eq!(tool.name(), "read_file");
}

#[test]
fn tool_resolver_unknown_function() {
    let resolver = ToolResolver::new();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(resolver.resolve(&ToolDecl::function("nonexistent_tool", "")));
    assert!(result.is_err());
}

// ── PropertySchema Tests ──

#[test]
fn property_schema_find() {
    let schema = PropertySchema::new(vec![
        Property {
            name: "question".to_string(),
            kind: PropertyType::String,
            description: "The question".to_string(),
            required: true,
            default: None,
            example: None,
            enum_values: vec![],
        },
    ]);
    let prop = schema.find_property("question").unwrap();
    assert_eq!(prop.name, "question");
    assert!(prop.required);
    assert!(schema.find_property("nonexistent").is_none());
}

// ── Template Tests ──

#[test]
fn template_mustache_prompty() {
    let t = Template::mustache_prompty();
    assert!(t.is_mustache());
    assert!(t.is_prompty());
}

// ── ExpressionEngine Tests ──

#[test]
fn expression_engine_is_expression() {
    use rust_agent_decl::ExpressionEngine;
    assert!(ExpressionEngine::is_expression("=test"));
    assert!(!ExpressionEngine::is_expression("test"));
}

#[test]
fn expression_engine_resolve_env() {
    use rust_agent_decl::ExpressionEngine;
    std::env::set_var("RUST_AGENT_DECL_TEST", "test123");
    assert_eq!(ExpressionEngine::resolve_env("$RUST_AGENT_DECL_TEST"), Some("test123".to_string()));
    assert_eq!(ExpressionEngine::resolve_env("$NONEXISTENT_XYZ123"), None);
    std::env::remove_var("RUST_AGENT_DECL_TEST");
}

// ── AgentDefinition Tests ──

#[test]
fn agent_definition_builder() {
    let model = Model::new("gpt-4o").with_connection(Connection::api_key("sk-test"));
    let def = AgentDefinition::new_prompt("test", model);
    assert_eq!(def.name, "test");
    assert!(def.is_prompt());
}

#[test]
fn agent_definition_workflow_builder() {
    let def = AgentDefinition::new_workflow("test-wf", "OnConversationStart", "wf1");
    assert_eq!(def.name, "test-wf");
    assert!(def.is_workflow());
}

// ── Error Tests ──

#[test]
fn parse_invalid_json() {
    let result = AgentDocument::from_json_str("not json");
    assert!(result.is_err());
}

#[test]
fn agent_document_into_definition() {
    let json = r#"{"kind":"prompt","name":"test","model":{"id":"gpt-4o","connection":{"kind":"key","api_key":"sk"}}}"#;
    let doc = AgentDocument::from_json_str(json).unwrap();
    let _def = doc.into_definition().expect("Should extract definition");
}
