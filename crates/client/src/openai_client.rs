use async_trait::async_trait;

use rust_agent_core::{
    AgentResponseUpdate, BoxStream, ChatClientRunOptions, ChatMessage, IChatClient, Result,
};

use crate::chat_client::ChatClient;
use crate::options::ChatClientOptions;
use crate::types::ModelListEntry;
use crate::usage::UsageFormat;

/// 实现 IChatClient 的 OpenAI 聊天客户端。
///
/// 组合通用 `ChatClient` 用于 HTTP/SSE 传输，并添加 OpenAI 特定的 API 能力。
pub struct OpenAiChatClient {
    inner: ChatClient,
}

impl OpenAiChatClient {
    pub fn new(options: ChatClientOptions) -> Result<Self> {
        Ok(Self {
            inner: ChatClient::new(options)?,
        })
    }

    pub fn inner(&self) -> &ChatClient {
        &self.inner
    }

    /// 列出可用的模型。
    /// 发送 GET 请求 `{api_base}/models` → `{ "object": "list", "data": [...] }`
    pub async fn list_models(&self) -> Result<Vec<ModelListEntry>> {
        let url = format!(
            "{}/models",
            self.inner.options().api_base.trim_end_matches('/')
        );
        let client = reqwest::Client::new();
        let resp = client
            .get(&url)
            .header(
                "Authorization",
                format!("Bearer {}", self.inner.options().api_key),
            )
            .send()
            .await
            .map_err(|e| {
                rust_agent_core::AgentError::ChatClientError(format!("list_models failed: {}", e))
            })?;

        let json: serde_json::Value = resp.json().await.map_err(|e| {
            rust_agent_core::AgentError::ChatClientError(format!(
                "list_models parse error: {}",
                e
            ))
        })?;

        let entries: Vec<ModelListEntry> = json["data"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| serde_json::from_value(item.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();

        Ok(entries)
    }
}

#[async_trait]
impl IChatClient for OpenAiChatClient {
    async fn run(
        &self,
        messages: &[ChatMessage],
        options: ChatClientRunOptions,
    ) -> Result<BoxStream<'static, Result<AgentResponseUpdate>>> {
        // OpenAI-specific: parses usage with nested `prompt_tokens_details.cached_tokens`
        self.inner.chat_stream(messages, &options, UsageFormat::OpenAI).await
    }

    fn model_id(&self) -> &str {
        self.inner.model_id()
    }
}
