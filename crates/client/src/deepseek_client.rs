use async_trait::async_trait;

use rust_agent_core::{
    AgentResponseUpdate, BoxStream, ChatClientRunOptions, ChatMessage, IChatClient, Result,
};

use crate::chat_client::ChatClient;
use crate::options::ChatClientOptions;
use crate::types::ModelListEntry;
use crate::usage::UsageFormat;

/// 实现 IChatClient 的 DeepSeek 聊天客户端。
///
/// DeepSeek 的 API 与 OpenAI 兼容，但有以下区别：
/// - 基础 URL 为 `https://api.deepseek.com`（无 `/v1` 前缀）
/// - 通过 `ChatAgentRunOptions::with_thinking()` 支持 `thinking` 模式
/// - 在流式增量中返回 `reasoning_content`
/// - 在用量数据中返回 `prompt_cache_hit_tokens` / `prompt_cache_miss_tokens`
///
/// 组合通用 `ChatClient` 用于 HTTP/SSE 传输，并添加 DeepSeek 特定的 API 能力。
///
/// 每次调用的选项（思考模式、推理努力等）通过 `ChatAgentRunOptions` 配置，而非此客户端，保持接口简洁。
pub struct DeepSeekChatClient {
    inner: ChatClient,
}

impl DeepSeekChatClient {
    pub fn new(options: ChatClientOptions) -> Result<Self> {
        Ok(Self {
            inner: ChatClient::new(options)?,
        })
    }

    pub fn inner(&self) -> &ChatClient {
        &self.inner
    }

    /// 列出可用的 DeepSeek 模型。
    /// 发送 GET 请求 `https://api.deepseek.com/models` → `{ "object": "list", "data": [...] }`
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
                rust_agent_core::AgentError::ChatClientError(format!(
                    "deepseek list_models failed: {}",
                    e
                ))
            })?;

        let json: serde_json::Value = resp.json().await.map_err(|e| {
            rust_agent_core::AgentError::ChatClientError(format!(
                "deepseek list_models parse error: {}",
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
impl IChatClient for DeepSeekChatClient {
    async fn run(
        &self,
        messages: &[ChatMessage],
        options: ChatClientRunOptions,
    ) -> Result<BoxStream<'static, Result<AgentResponseUpdate>>> {
        // DeepSeek-specific: parses usage with top-level `prompt_cache_hit_tokens` / `prompt_cache_miss_tokens`
        self.inner.chat_stream(messages, &options, UsageFormat::DeepSeek).await
    }

    fn model_id(&self) -> &str {
        self.inner.model_id()
    }
}
