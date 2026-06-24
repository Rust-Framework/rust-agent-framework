//! Manual integration probe for Agnes AI tool-loop compatibility.
//! Run: AGNES_API_KEY=sk-... cargo test -p rust-agent-client agnes_tool_loop -- --ignored --nocapture

use rust_agent_client::{ChatClient, ChatClientOptions};
use rust_agent_core::{ChatClientRunOptions, ChatMessage, MessageRole, ToolCall};
use futures_util::StreamExt;

#[tokio::test]
#[ignore]
async fn agnes_tool_loop_round_trip() {
    let key = std::env::var("AGNES_API_KEY").expect("AGNES_API_KEY");
    let client = ChatClient::new(ChatClientOptions {
        api_base: "https://apihub.agnes-ai.com/v1".into(),
        api_key: key,
        model: "agnes-2.0-flash".into(),
        ..Default::default()
    })
    .unwrap();

    let messages = vec![
        ChatMessage::system("You are helpful."),
        ChatMessage::user("list files"),
        ChatMessage {
            role: MessageRole::Assistant,
            content: String::new(),
            name: None,
            tool_calls: Some(vec![ToolCall {
                id: "call_test_1".into(),
                name: "list_files".into(),
                arguments: serde_json::json!({"path": "."}),
            }]),
            tool_call_id: None,
            source: None,
        },
        ChatMessage::tool(r#"{"ok":true,"data":{"count":1}}"#, "call_test_1"),
    ];

    let mut opts = ChatClientRunOptions::default();
    opts.tools = vec![serde_json::json!({
        "type": "function",
        "function": {
            "name": "list_files",
            "description": "List files",
            "parameters": {
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            }
        }
    })];

    let mut stream = client
        .chat_stream(&messages, &opts, rust_agent_client::usage::UsageFormat::OpenAI)
        .await
        .expect("round 2 should succeed");

    let mut chunks = 0;
    while let Some(item) = stream.next().await {
        item.expect("chunk");
        chunks += 1;
    }
    assert!(chunks > 0);
}
