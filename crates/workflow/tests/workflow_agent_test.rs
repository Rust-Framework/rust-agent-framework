//! WorkflowAgent 流式运行测试 — 验证工作流完成后流能正常结束

use std::sync::Arc;

use async_trait::async_trait;
use futures_util::StreamExt;
use rust_agent_core::{
    AgentId, AgentMetadata, AgentResponseResult, AgentRunOptions, BoxStream, ChatMessage,
    Content, FinishReason, IAgent, ISession, Result, TextContent,
};
use rust_agent_workflow::{ContextFunctionExecutor, HandlerResult, IExecutor, WorkflowAgent, WorkflowBuilder};

struct StubAgent {
    reply: String,
}

#[async_trait]
impl IAgent for StubAgent {
    fn id(&self) -> &AgentId {
        static ID: std::sync::OnceLock<AgentId> = std::sync::OnceLock::new();
        ID.get_or_init(|| AgentId::new("stub"))
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
        let (tx, rx) = tokio::sync::mpsc::channel(2);
        tokio::spawn(async move {
            let _ = tx
                .send(Ok(AgentResponseResult {
                    id: Some("stub".into()),
                    model: None,
                    finish_reason: Some(FinishReason::Stop),
                    contents: vec![Content::Text(TextContent {
                        delta: reply,
                        meta: rust_agent_core::ResponseMetadata {
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
async fn workflow_agent_stream_completes_after_single_invoke_node() {
    let stub = Arc::new(StubAgent {
        reply: "hello-from-stub".into(),
    });

    let invoke = Arc::new(ContextFunctionExecutor::new("invoke", {
        let stub = Arc::clone(&stub);
        move |_msg, ctx, progress| {
            let stub = Arc::clone(&stub);
            async move {
                let stream = stub.run(vec![ChatMessage::user("hi")], None, None).await?;
                futures_util::pin_mut!(stream);
                let mut text = String::new();
                while let Some(item) = stream.next().await {
                    let result = item?;
                    for content in &result.contents {
                        if let Content::Text(t) = content {
                            text.push_str(&t.delta);
                            let _ = progress.send(rust_agent_workflow::NodeProgress::TextDelta(
                                t.delta.clone(),
                            ));
                        }
                    }
                }
                let msg = Arc::new(ChatMessage::assistant(&text));
                ctx.yield_output(msg.clone()).await?;
                Ok(HandlerResult::Messages(vec![msg]))
            }
        }
    })) as Arc<dyn IExecutor>;

    let graph = WorkflowBuilder::new()
        .add_node("invoke", invoke)
        .set_start("invoke")
        .with_output_from("invoke")
        .build()
        .expect("graph builds");

    let agent = WorkflowAgent::new(graph);
    let stream = agent
        .run(vec![ChatMessage::user("trigger")], None, None)
        .await
        .expect("agent runs");

    futures_util::pin_mut!(stream);
    let collect = async {
        let mut saw_text = false;
        while let Some(chunk) = stream.next().await {
            let result = chunk.expect("chunk ok");
            for content in &result.contents {
                if let Content::Text(t) = content {
                    if t.delta.contains("hello-from-stub") {
                        saw_text = true;
                    }
                }
            }
        }
        saw_text
    };

    let saw_text = tokio::time::timeout(std::time::Duration::from_secs(5), collect)
        .await
        .expect("stream should finish within 5s");
    assert!(saw_text);
}
