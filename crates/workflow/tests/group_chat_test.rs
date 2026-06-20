//! GroupChat selector 与 termination 单元测试

use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::{
    AgentId, AgentMetadata, AgentResponseResult, AgentRunOptions, BoxStream, ChatMessage,
    Content, FinishReason, IAgent, ISession, Result, TextContent,
};
use rust_agent_workflow::{
    FixedOrderSelector, ISpeakerSelector, ITerminationCondition, KeywordTermination,
    MaxRoundsTermination, RoundRobinSelector,
};

struct EchoAgent {
    id: AgentId,
    label: String,
}

#[async_trait]
impl IAgent for EchoAgent {
    fn id(&self) -> &AgentId {
        &self.id
    }

    fn metadata(&self) -> &AgentMetadata {
        static META: std::sync::OnceLock<AgentMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| AgentMetadata::new("EchoAgent", "echo"))
    }

    async fn run(
        &self,
        _messages: Vec<ChatMessage>,
        _session: Option<Arc<dyn ISession>>,
        _options: Option<AgentRunOptions>,
    ) -> Result<BoxStream<'static, Result<AgentResponseResult>>> {
        let label = self.label.clone();
        let (tx, rx) = tokio::sync::mpsc::channel(2);
        tokio::spawn(async move {
            let _ = tx
                .send(Ok(AgentResponseResult {
                    id: Some(label.clone()),
                    model: None,
                    finish_reason: Some(FinishReason::Stop),
                    contents: vec![Content::Text(TextContent {
                        delta: label,
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

fn participants() -> Vec<Arc<dyn IAgent>> {
    vec![
        Arc::new(EchoAgent {
            id: AgentId::new("a"),
            label: "A".into(),
        }),
        Arc::new(EchoAgent {
            id: AgentId::new("b"),
            label: "B".into(),
        }),
        Arc::new(EchoAgent {
            id: AgentId::new("c"),
            label: "C".into(),
        }),
    ]
}

#[tokio::test]
async fn round_robin_selector_cycles_participants() {
    let selector = RoundRobinSelector::new();
    let parts = participants();
    let history: Vec<ChatMessage> = vec![];

    let i0 = selector.select_next(&history, &parts).await.unwrap();
    let i1 = selector.select_next(&history, &parts).await.unwrap();
    let i2 = selector.select_next(&history, &parts).await.unwrap();
    let i3 = selector.select_next(&history, &parts).await.unwrap();

    assert_eq!([i0, i1, i2, i3], [0, 1, 2, 0]);
}

#[tokio::test]
async fn fixed_order_selector_follows_configured_order() {
    let selector = FixedOrderSelector::new(vec![2, 0, 1]);
    let parts = participants();
    let history: Vec<ChatMessage> = vec![];

    assert_eq!(selector.select_next(&history, &parts).await.unwrap(), 2);
    assert_eq!(selector.select_next(&history, &parts).await.unwrap(), 0);
    assert_eq!(selector.select_next(&history, &parts).await.unwrap(), 1);
    assert_eq!(selector.select_next(&history, &parts).await.unwrap(), 2);
}

#[test]
fn max_rounds_termination_stops_after_limit() {
    let term = MaxRoundsTermination::new(2);
    let history = vec![
        ChatMessage::assistant("one"),
        ChatMessage::assistant("two"),
    ];
    assert!(term.should_terminate(&history));

    let history = vec![ChatMessage::assistant("one")];
    assert!(!term.should_terminate(&history));
}

#[test]
fn keyword_termination_detects_assistant_message() {
    let term = KeywordTermination::new(vec!["FINAL".into()]);
    let history = vec![ChatMessage::assistant("here is the FINAL answer")];
    assert!(term.should_terminate(&history));

    let history = vec![ChatMessage::user("FINAL")];
    assert!(!term.should_terminate(&history));
}

#[test]
fn keyword_termination_is_case_insensitive() {
    let term = KeywordTermination::new(vec!["done".into()]);
    let history = vec![ChatMessage::assistant("All DONE.")];
    assert!(term.should_terminate(&history));
}

#[tokio::test]
async fn group_chat_runner_respects_max_rounds() {
    use futures_util::StreamExt;
    use rust_agent_workflow::{GroupChatWorkflowBuilder, MaxRoundsTermination, RoundRobinSelector};

    let participants = participants();
    let wf = GroupChatWorkflowBuilder::new()
        .add_participant(participants[0].clone())
        .add_participant(participants[1].clone())
        .selector(Arc::new(RoundRobinSelector::new()))
        .termination(Arc::new(MaxRoundsTermination::new(2)))
        .build()
        .expect("group chat builds");

    let agent = wf.as_agent();
    let stream = agent
        .run(vec![ChatMessage::user("discuss")], None, None)
        .await
        .expect("group chat runs");

    futures_util::pin_mut!(stream);
    let mut turns = 0usize;
    while let Some(chunk) = stream.next().await {
        chunk.expect("chunk ok");
        turns += 1;
    }
    assert_eq!(turns, 3, "two speaker turns plus final stop frame");
}
