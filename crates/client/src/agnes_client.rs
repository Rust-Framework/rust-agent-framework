//! Agnes AI chat client — OpenAI-compatible gateway at `apihub.agnes-ai.com`.
//!
//! Docs: https://agnes-ai.com/doc/agnes-20-flash
//! - Endpoint: `POST /v1/chat/completions`
//! - Thinking / reasoning via `thinking` in request body (`AgentRunOptions::with_thinking`)
//! - SSE `reasoning_content` deltas (same wire shape as DeepSeek)
//! - Reference limits: 256K context, 64K max output (`agnes-2.0-flash`)

use async_trait::async_trait;

use rust_agent_core::{
    AgentResponseUpdate, BoxStream, ChatClientRunOptions, ChatMessage, IChatClient, ModelMetadata,
    Result,
};

use crate::chat_client::ChatClient;
use crate::options::ChatClientOptions;
use crate::types::ModelListEntry;
use crate::usage::UsageFormat;

pub const AGNES_DEFAULT_API_BASE: &str = "https://apihub.agnes-ai.com/v1";

/// Agnes AI 聊天客户端（OpenAI 兼容网关 + Agnes 专用用量/元数据）。
pub struct AgnesChatClient {
    inner: ChatClient,
}

impl AgnesChatClient {
    pub fn new(mut options: ChatClientOptions) -> Result<Self> {
        if options.api_base.is_empty()
            || options.api_base == "https://api.openai.com/v1"
        {
            options.api_base = AGNES_DEFAULT_API_BASE.to_string();
        }
        if options.model_metadata.is_none() {
            options.model_metadata = Some(agnes_model_metadata(&options.model));
        }
        Ok(Self {
            inner: ChatClient::new(options)?,
        })
    }

    pub fn inner(&self) -> &ChatClient {
        &self.inner
    }

    /// `GET /v1/models`
    pub async fn list_models(&self) -> Result<Vec<ModelListEntry>> {
        let url = format!(
            "{}/models",
            self.inner.options().api_base.trim_end_matches('/')
        );
        let resp = self
            .inner
            .http()
            .get(&url)
            .header(
                "Authorization",
                format!("Bearer {}", self.inner.options().api_key),
            )
            .send()
            .await
            .map_err(|e| {
                rust_agent_core::AgentError::ChatClientError(format!(
                    "agnes list_models failed: {}",
                    e
                ))
            })?;

        let json: serde_json::Value = resp.json().await.map_err(|e| {
            rust_agent_core::AgentError::ChatClientError(format!(
                "agnes list_models parse error: {}",
                e
            ))
        })?;

        Ok(json["data"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| serde_json::from_value(item.clone()).ok())
                    .collect()
            })
            .unwrap_or_default())
    }
}

#[async_trait]
impl IChatClient for AgnesChatClient {
    async fn run(
        &self,
        messages: &[ChatMessage],
        options: ChatClientRunOptions,
    ) -> Result<BoxStream<'static, Result<AgentResponseUpdate>>> {
        self.inner
            .chat_stream(messages, &options, UsageFormat::Agnes)
            .await
    }

    fn model_id(&self) -> &str {
        self.inner.model_id()
    }

    fn model_metadata(&self) -> Option<&ModelMetadata> {
        self.inner.model_metadata()
    }
}

/// Agnes 模型参考规格（来自官方 MODEL_CATALOG，2026-06-22）。
pub fn agnes_model_metadata(model_id: &str) -> ModelMetadata {
    let (context, max_output) = match model_id {
        id if id.starts_with("agnes-2.0") || id.starts_with("agnes-1.5") => (256_000, 64_000),
        _ => (128_000, 8_192),
    };
    ModelMetadata::new(model_id, context, max_output)
}
