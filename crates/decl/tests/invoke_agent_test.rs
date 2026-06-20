#![cfg(feature = "yaml")]

//! InvokeAgent workflow 编译与运行集成测试

use std::sync::Arc;

use async_trait::async_trait;
use futures_util::StreamExt;
use rust_agent_core::{
    AgentId, AgentMetadata, AgentResponseResult, AgentRunOptions, BoxStream, ChatMessage,
    Content, FinishReason, IAgent, ISession, ResponseMetadata, Result, TextContent,
};
use rust_agent_decl::DeclAgentBuilder;

struct StubAgent {
    id: AgentId,
    reply: String,
}

#[async_trait]
impl IAgent for StubAgent {
    fn id(&self) -> &AgentId {
        &self.id
    }

    fn metadata(&self) -> &AgentMetadata {
        static META: std::sync::OnceLock<AgentMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| AgentMetadata::new("StubAgent", "stub"))
    }

    async fn run(
        &self,
        _messages: Vec<ChatMessage>,
        _session: Option<Arc<dyn ISession>>,
        _options: Option<AgentRunOptions>,
    ) -> Result<BoxStream<'static, Result<AgentResponseResult>>> {
        let reply = self.reply.clone();
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        tokio::spawn(async move {
            let _ = tx
                .send(Ok(AgentResponseResult {
                    id: Some("stub".to_string()),
                    model: None,
                    finish_reason: Some(FinishReason::Stop),
                    contents: vec![Content::Text(TextContent {
                        delta: reply,
                        meta: ResponseMetadata {
                            agent_id: None,
                            model_id: None,
                            executor_id: None,
                            timestamp: chrono::Utc::now(),
                            properties: Default::default(),
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

#[tokio::test]
async fn invoke_agent_workflow_builds_with_registered_agent() {
    let stub = Arc::new(StubAgent {
        id: AgentId::new("helper"),
        reply: "stub-response".to_string(),
    });

    let yaml = r#"
kind: workflow
name: invoke-test
trigger:
  kind: OnConversationStart
  id: start
  actions:
    - kind: InvokeAgent
      id: call_helper
      agent:
        name: helper
      input:
        messages:
          - role: user
            content: hello
      output:
        responseObject: Local.response
"#;

    let agent = DeclAgentBuilder::new()
        .from_yaml_str(yaml)
        .with_workflow_agent("helper", stub)
        .build()
        .await
        .expect("invoke workflow builds");

    assert_eq!(agent.id().to_string(), "invoke-test");
}

#[tokio::test]
async fn invoke_agent_workflow_runs_registered_agent() {
    let stub = Arc::new(StubAgent {
        id: AgentId::new("helper"),
        reply: "stub-response".to_string(),
    });

    let yaml = r#"
kind: workflow
name: invoke-run-test
trigger:
  kind: OnConversationStart
  id: start
  actions:
    - kind: InvokeAgent
      id: call_helper
      agent:
        name: helper
      input:
        messages:
          - role: user
            content: hello
"#;

    let agent = DeclAgentBuilder::new()
        .from_yaml_str(yaml)
        .with_workflow_agent("helper", stub)
        .build()
        .await
        .expect("invoke workflow builds");

    let stream = agent
        .run(vec![ChatMessage::user("trigger")], None, None)
        .await
        .expect("workflow runs");

    futures_util::pin_mut!(stream);
    let mut saw_stub = false;
    while let Some(chunk) = stream.next().await {
        let result = chunk.expect("stream chunk");
        for content in &result.contents {
            if let Content::Text(t) = content {
                if t.delta.contains("stub-response") {
                    saw_stub = true;
                }
            }
        }
    }
    assert!(saw_stub, "expected stub agent response in workflow output");
}
