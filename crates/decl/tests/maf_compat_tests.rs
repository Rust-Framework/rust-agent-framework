//! MAF compatibility tests — verifies that MAF-compatible YAML files
//! are correctly parsed by rust-agent-decl.

#[cfg(feature = "yaml")]
mod tests {
    use rust_agent_decl::{AgentDocument, AgentKindData};

    #[test]
    fn parse_prompt_agent_yaml() {
        let yaml = include_str!("fixtures/prompt_agent.yaml");
        let doc = AgentDocument::from_yaml_str(yaml).expect("Must parse MAF prompt agent YAML");

        match &doc {
            AgentDocument::Definition(def) => {
                assert_eq!(def.name, "Assistant");
                assert_eq!(def.display_name.as_deref(), Some("Helpful Assistant"));
                assert_eq!(def.description, "A helpful assistant that answers questions in a JSON format.");
                assert!(matches!(def.kind_data, AgentKindData::Prompt(_)));

                if let AgentKindData::Prompt(data) = &def.kind_data {
                    assert_eq!(data.model.id, "gpt-4o");
                    assert!(!data.instructions.is_empty());
                }
            }
            AgentDocument::Manifest(_) => panic!("Expected raw Definition, got Manifest"),
        }
    }

    #[test]
    fn parse_prompt_agent_with_tools() {
        let yaml = include_str!("fixtures/prompt_agent_with_tools.yaml");
        let doc = AgentDocument::from_yaml_str(yaml).expect("Must parse prompt agent with tools");

        if let AgentDocument::Definition(def) = &doc {
            if let AgentKindData::Prompt(data) = &def.kind_data {
                assert_eq!(data.tools.len(), 2);
                assert_eq!(data.tools[0].kind_str(), "web");
                assert_eq!(data.tools[0].name(), Some("web_search"));
                assert_eq!(data.tools[1].kind_str(), "file");
                assert_eq!(data.tools[1].name(), Some("read_file"));
            }
        }
    }

    #[test]
    fn parse_workflow_basic() {
        let yaml = include_str!("fixtures/workflow_basic.yaml");
        let doc = AgentDocument::from_yaml_str(yaml).expect("Must parse MAF workflow YAML");

        if let AgentDocument::Definition(def) = &doc {
            assert_eq!(def.name, "greeting-workflow");
            if let AgentKindData::Workflow(data) = &def.kind_data {
                assert_eq!(data.trigger.kind, "OnConversationStart");
                assert_eq!(data.trigger.id, "greeting_workflow");
                assert_eq!(data.trigger.actions.len(), 3);
                assert_eq!(data.trigger.actions[0].kind_str(), "SetVariable");
                assert_eq!(data.trigger.actions[1].kind_str(), "SetVariable");
                assert_eq!(data.trigger.actions[2].kind_str(), "SendActivity");
            } else {
                panic!("Expected Workflow variant");
            }
        }
    }

    #[test]
    fn parse_workflow_with_if() {
        let yaml = include_str!("fixtures/workflow_with_if.yaml");
        let doc = AgentDocument::from_yaml_str(yaml).expect("Must parse workflow with If");

        if let AgentDocument::Definition(def) = &doc {
            if let AgentKindData::Workflow(data) = &def.kind_data {
                assert_eq!(data.trigger.actions.len(), 2);
                assert_eq!(data.trigger.actions[0].kind_str(), "SetVariable");
                assert_eq!(data.trigger.actions[1].kind_str(), "If");
            }
        }
    }

    #[test]
    fn parse_container_agent() {
        let yaml = include_str!("fixtures/container_agent.yaml");
        let doc = AgentDocument::from_yaml_str(yaml).expect("Must parse container agent YAML");

        if let AgentDocument::Definition(def) = &doc {
            assert_eq!(def.name, "my-hosted-agent");
            if let AgentKindData::Container(data) = &def.kind_data {
                assert_eq!(data.protocols.len(), 1);
                assert_eq!(data.protocols[0].protocol, "responses");
                assert_eq!(data.image.as_deref(), Some("myregistry.azurecr.io/my-agent"));
                assert_eq!(data.resources.cpu, "1");
                assert_eq!(data.resources.memory, "2Gi");
            }
        }
    }

    #[test]
    fn parse_agent_manifest() {
        let yaml = include_str!("fixtures/agent_manifest.yaml");
        let doc = AgentDocument::from_yaml_str(yaml).expect("Must parse agent manifest YAML");

        match &doc {
            AgentDocument::Manifest(m) => {
                assert_eq!(m.name, "my-agent-manifest");
                assert_eq!(m.template.name, "MyAgent");
                assert_eq!(m.resources.len(), 1);
                assert_eq!(m.resources[0].name, "chat");
            }
            AgentDocument::Definition(_) => panic!("Expected Manifest, got Definition"),
        }
    }

    #[test]
    fn parse_all_member_access() {
        // Verifies all struct member access works after parsing
        let yaml = include_str!("fixtures/prompt_agent.yaml");
        let doc = AgentDocument::from_yaml_str(yaml).unwrap();

        let def = doc.inner_definition();

        // Test accessors
        assert!(def.is_prompt());
        assert!(!def.is_workflow());
        assert!(!def.is_container());

        // Test document conversion
        let clone = doc.clone();
        let _def2 = clone.into_definition().expect("Should extract definition");
    }

    #[test]
    fn document_roundtrip_yaml() {
        let yaml = include_str!("fixtures/prompt_agent.yaml");
        let doc = AgentDocument::from_yaml_str(yaml).unwrap();
        let serialized = doc.to_yaml_string().unwrap();
        let doc2 = AgentDocument::from_yaml_str(&serialized).unwrap();

        let def1 = doc.inner_definition();
        let def2 = doc2.inner_definition();
        assert_eq!(def1.name, def2.name);
        assert_eq!(def1.display_name, def2.display_name);
    }
}
