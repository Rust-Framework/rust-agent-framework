use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::{AgentError, AgentResponseUpdate, BoxStream, ChatMessage, ITool, ModelMetadata, Result};
use crate::tool::ToolApprovalResponse;

/// Per-call run options for `IChatClient::run()`, following MAF's pattern.
///
/// Overrides the client's defaults for a single call.
/// All fields are `Option` — `None` means "use the client's default".
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ChatClientRunOptions {
    /// Override max_tokens for this call.
    pub max_tokens: Option<u32>,
    /// Override temperature for this call.
    pub temperature: Option<f32>,
    /// Override top_p for this call.
    pub top_p: Option<f32>,
    /// Override stop sequences for this call.
    pub stop: Option<Vec<String>>,
    /// Extra JSON fields merged into the request body top-level
    /// for this call only (e.g. `{"thinking": {"type": "enabled"}}`).
    pub extra_body: HashMap<String, serde_json::Value>,
    /// Tool definitions in OpenAI function-calling format.
    /// Each entry is a JSON object like:
    /// ```json
    /// { "type": "function", "function": { "name": "...", "description": "...", "parameters": {...} } }
    /// ```
    pub tools: Vec<serde_json::Value>,
    /// Allow parallel tool calls. When `Some(true)`, the LLM may emit multiple
    /// tool calls in a single response. When `Some(false)`, tool calls are
    /// serialized. `None` means use the provider default (typically enabled).
    /// Maps to OpenAI's `parallel_tool_calls` parameter.
    pub parallel_tool_calls: Option<bool>,
    /// Provider-injected tool instances for execution.
    ///
    /// Follows MAF's pattern where context providers inject tools into
    /// `ChatOptions.Tools`, and `FunctionInvokingChatClient` reads them
    /// at execution time. These tools supplement the statically-registered
    /// tools in `FunctionInvokingChatClient` and are resolved by name
    /// during the tool-calling loop.
    ///
    /// Unlike `self.tools` (JSON schemas sent to the LLM), this field
    /// carries the actual `Arc<dyn ITool>` instances for invocation.
    #[serde(skip)]
    pub provider_tools: Vec<Arc<dyn ITool>>,
    /// Tool approval responses propagated from `AgentRunOptions` for
    /// resuming after an approval pause.
    #[serde(skip)]
    pub tool_approval_responses: Vec<ToolApprovalResponse>,
    /// Cancel flag propagated from `AgentRunOptions`.
    #[serde(skip)]
    pub cancelled: Option<Arc<std::sync::atomic::AtomicBool>>,
}

// Manual Debug impl — `dyn ITool` doesn't impl Debug, so we skip provider_tools.
impl std::fmt::Debug for ChatClientRunOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatClientRunOptions")
            .field("max_tokens", &self.max_tokens)
            .field("temperature", &self.temperature)
            .field("top_p", &self.top_p)
            .field("stop", &self.stop)
            .field("extra_body", &self.extra_body)
            .field("tools", &self.tools)
            .field("parallel_tool_calls", &self.parallel_tool_calls)
            .field("provider_tools", &format_args!("[{} tools]", self.provider_tools.len()))
            .field("tool_approval_responses", &self.tool_approval_responses.len())
            .finish()
    }
}

impl ChatClientRunOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn with_extra_body(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.extra_body.insert(key.into(), value);
        self
    }

    pub fn with_tools(mut self, tools: Vec<serde_json::Value>) -> Self {
        self.tools = tools;
        self
    }
}

/// Chat client interface following MAF's ChatClient abstraction.
///
/// A thin wrapper over LLM provider APIs.
/// Only streaming output is supported.
#[async_trait]
pub trait IChatClient: Send + Sync {
    /// Run chat completion and produce a stream of update deltas.
    ///
    /// `options` allows per-call overrides (temperature, extra_body, etc.)
    /// without mutating the client's persistent configuration.
    /// Pass `Default::default()` for standard behaviour.
    async fn run(
        &self,
        messages: &[ChatMessage],
        options: ChatClientRunOptions,
    ) -> Result<BoxStream<'static, Result<AgentResponseUpdate>>>;

    /// The model identifier used by this client.
    fn model_id(&self) -> &str;

    /// Model metadata describing capability boundaries (context window, max output).
    ///
    /// Used by compression strategies and the framework to enforce token limits.
    /// Returns `None` when model boundaries are unknown (default).
    /// Concrete implementations should override this.
    fn model_metadata(&self) -> Option<&ModelMetadata> {
        None
    }
}

/// ChatClient 装饰器基类，参照 MAF 的 DelegatingChatClient
///
/// 所有未重写的方法透传给 inner client。
/// 自定义装饰器应继承此结构体并重写需要拦截的方法。
pub struct DelegatingChatClient {
    inner: Arc<dyn IChatClient>,
}

impl DelegatingChatClient {
    pub fn new(inner: Arc<dyn IChatClient>) -> Self {
        Self { inner }
    }

    pub fn inner(&self) -> &Arc<dyn IChatClient> {
        &self.inner
    }
}

#[async_trait]
impl IChatClient for DelegatingChatClient {
    async fn run(
        &self,
        messages: &[ChatMessage],
        options: ChatClientRunOptions,
    ) -> Result<BoxStream<'static, Result<AgentResponseUpdate>>> {
        self.inner.run(messages, options).await
    }

    fn model_id(&self) -> &str {
        self.inner.model_id()
    }

    fn model_metadata(&self) -> Option<&ModelMetadata> {
        self.inner.model_metadata()
    }
}

/// ChatClient 管道构建器，参照 MAF 的 ChatClientBuilder
///
/// 按注册顺序包装装饰器，最终形成管道链。
/// 装饰器按注册顺序依次包装 leaf client：
/// `decorators[0](decorators[1](...(leaf)...))`
///
/// ## 使用方式
///
/// ```ignore
/// let pipeline = ChatClientBuilder::new()
///     .leaf(Arc::new(my_client))
///     .use_decorator(Box::new(|inner| Arc::new(FunctionInvokingChatClient::new(inner, tools))))
///     .build()?;
/// ```
pub struct ChatClientBuilder {
    decorators: Vec<Box<dyn Fn(Arc<dyn IChatClient>) -> Arc<dyn IChatClient> + Send + Sync>>,
    leaf: Option<Arc<dyn IChatClient>>,
}

impl ChatClientBuilder {
    pub fn new() -> Self {
        Self {
            decorators: Vec::new(),
            leaf: None,
        }
    }

    /// 设置叶子 ChatClient（实际的 LLM 服务客户端）
    pub fn leaf(mut self, client: Arc<dyn IChatClient>) -> Self {
        self.leaf = Some(client);
        self
    }

    /// 添加装饰器工厂
    ///
    /// 装饰器按注册顺序包装：先注册的装饰器在最外层。
    pub fn use_decorator(
        mut self,
        factory: Box<dyn Fn(Arc<dyn IChatClient>) -> Arc<dyn IChatClient> + Send + Sync>,
    ) -> Self {
        self.decorators.push(factory);
        self
    }

    /// 构建管道：decorators[0] 包装 leaf，decorators[1] 包装上一层，以此类推
    pub fn build(self) -> Result<Arc<dyn IChatClient>> {
        let mut client = self.leaf.ok_or_else(|| {
            AgentError::ConfigError("leaf IChatClient is required".into())
        })?;
        for factory in self.decorators {
            client = factory(client);
        }
        Ok(client)
    }
}

impl Default for ChatClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}
