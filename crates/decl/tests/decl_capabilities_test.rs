#![cfg(feature = "yaml")]

//! decl 非侵入扩展能力构建测试（external_loop / InvokeFunctionTool / magentic / passOutput）

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::{ITool, Result, ToolResult};
use rust_agent_decl::DeclAgentBuilder;

struct EchoTool;

#[async_trait]
impl ITool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }

    fn description(&self) -> &str {
        "echoes input"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<ToolResult> {
        Ok(ToolResult {
            ok: true,
            data: Some(arguments.get("text").cloned().unwrap_or(serde_json::json!(""))),
            error: None,
        })
    }
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

#[tokio::test]
async fn invoke_function_tool_workflow_builds_with_registered_tool() {
    let yaml = r#"
kind: workflow
name: tool-invoke-test
trigger:
  kind: OnConversationStart
  id: start
  actions:
    - kind: InvokeFunctionTool
      id: call_echo
      functionName: echo
      arguments:
        text: hello
      output:
        result: Local.tool_out
"#;

    let agent = DeclAgentBuilder::new()
        .from_yaml_str(yaml)
        .with_tool("echo", |_args| Ok(Arc::new(EchoTool)))
        .build()
        .await
        .expect("InvokeFunctionTool workflow builds");

    assert_eq!(agent.id().to_string(), "tool-invoke-test");
}

#[tokio::test]
async fn invoke_agent_external_loop_workflow_builds() {
    let yaml = r#"
kind: workflow
name: external-loop-test
trigger:
  kind: OnConversationStart
  id: start
  actions:
    - kind: InvokeAgent
      id: loop_call
      agent:
        name: helper
      input:
        messages:
          - role: user
            content: start
        externalLoop:
          when: Local.continue == true
      output:
        autoSend: true
        responseObject: Local.response
"#;

    use rust_agent_core::{
        AgentId, AgentMetadata, AgentResponseResult, AgentRunOptions, BoxStream, ChatMessage,
        Content, FinishReason, IAgent, ISession, ResponseMetadata, TextContent,
    };

    struct StubAgent;

    #[async_trait]
    impl IAgent for StubAgent {
        fn id(&self) -> &AgentId {
            static ID: std::sync::OnceLock<AgentId> = std::sync::OnceLock::new();
            ID.get_or_init(|| AgentId::new("helper"))
        }

        fn metadata(&self) -> &AgentMetadata {
            static META: std::sync::OnceLock<AgentMetadata> = std::sync::OnceLock::new();
            META.get_or_init(|| AgentMetadata::new("Stub", "helper"))
        }

        async fn run(
            &self,
            _messages: Vec<ChatMessage>,
            _session: Option<Arc<dyn ISession>>,
            _options: Option<AgentRunOptions>,
        ) -> Result<BoxStream<'static, Result<AgentResponseResult>>> {
            let (tx, rx) = tokio::sync::mpsc::channel(2);
            tokio::spawn(async move {
                let _ = tx
                    .send(Ok(AgentResponseResult {
                        id: None,
                        model: None,
                        finish_reason: Some(FinishReason::Stop),
                        contents: vec![Content::Text(TextContent {
                            delta: "ok".into(),
                            meta: ResponseMetadata {
                                agent_id: None,
                                model_id: None,
                                executor_id: None,
                                timestamp: chrono::Utc::now(),
                                properties: HashMap::new(),
                            },
                        })],
                        events: vec![],
                    }))
                    .await;
            });
            Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
        }

        async fn reset(&self) -> Result<()> {
            Ok(())
        }
    }

    let agent = DeclAgentBuilder::new()
        .from_yaml_str(yaml)
        .with_workflow_agent("helper", Arc::new(StubAgent))
        .build()
        .await
        .expect("external_loop workflow builds");

    assert_eq!(agent.id().to_string(), "external-loop-test");
}

#[tokio::test]
async fn build_magentic_with_max_iterations() {
    let yaml = format!(
        r#"
kind: prompt
name: magentic-iter
model:
  id: gpt-4o
  connection:
    kind: key
    api_key: sk-test
instructions: orchestrator
metadata:
  orchestration:
    mode: magentic
    maxIterations: 5
subAgents:
{}
"#,
        minimal_sub_agent("worker")
    );

    DeclAgentBuilder::new()
        .from_yaml_str(&yaml)
        .build()
        .await
        .expect("magentic with maxIterations builds");
}

#[tokio::test]
async fn build_sequential_pass_output_false() {
    let yaml = format!(
        r#"
kind: prompt
name: seq-fixed
model:
  id: gpt-4o
  connection:
    kind: key
    api_key: sk-test
instructions: orchestrator
metadata:
  orchestration:
    mode: sequential
    passOutput: false
subAgents:
{}
{}
"#,
        minimal_sub_agent("a"),
        minimal_sub_agent("b")
    );

    DeclAgentBuilder::new()
        .from_yaml_str(&yaml)
        .build()
        .await
        .expect("sequential passOutput:false builds");
}

#[tokio::test]
async fn build_sequential_pass_output_true() {
    let yaml = format!(
        r#"
kind: prompt
name: seq-chained
model:
  id: gpt-4o
  connection:
    kind: key
    api_key: sk-test
instructions: orchestrator
metadata:
  orchestration:
    mode: sequential
    passOutput: true
subAgents:
{}
{}
"#,
        minimal_sub_agent("a"),
        minimal_sub_agent("b")
    );

    DeclAgentBuilder::new()
        .from_yaml_str(&yaml)
        .build()
        .await
        .expect("sequential passOutput:true builds");
}
