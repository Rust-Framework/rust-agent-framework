use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use crate::{AgentError, AgentResponseUpdate, BoxStream, ChatMessage, ITool, ModelMetadata, Result};
use crate::tool::ToolApprovalResponse;

/// `IChatClient::run()` 的单次调用运行选项，遵循 MAF 模式。
///
/// 覆盖客户端的默认配置（仅本次调用生效）。
/// 所有字段均为 `Option` — `None` 表示"使用客户端默认值"。
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ChatClientRunOptions {
    /// 本次调用的 max_tokens 覆盖值
    pub max_tokens: Option<u32>,
    /// 本次调用的 temperature 覆盖值
    pub temperature: Option<f32>,
    /// 本次调用的 top_p 覆盖值
    pub top_p: Option<f32>,
    /// 本次调用的停止序列覆盖值
    pub stop: Option<Vec<String>>,
    /// 额外 JSON 字段，合并到请求体顶层（仅本次调用）
    /// 例如：`{"thinking": {"type": "enabled"}}`
    pub extra_body: HashMap<String, serde_json::Value>,
    /// OpenAI 函数调用格式的工具定义
    /// 每个条目为 JSON 对象：
    /// ```json
    /// { "type": "function", "function": { "name": "...", "description": "...", "parameters": {...} } }
    /// ```
    pub tools: Vec<serde_json::Value>,
    /// 是否允许并行工具调用。`Some(true)` 表示 LLM 可一次发出多个工具调用，
    /// `Some(false)` 表示串行调用。`None` 表示使用提供商的默认值（通常启用）。
    /// 对应 OpenAI 的 `parallel_tool_calls` 参数。
    pub parallel_tool_calls: Option<bool>,
    /// Provider 注入的可执行工具实例
    ///
    /// 遵循 MAF 模式：Context Provider 将工具注入到 `ChatOptions.Tools`，
    /// `FunctionInvokingChatClient` 在运行时读取并执行。
    /// 这些工具补充了 `FunctionInvokingChatClient` 中静态注册的工具，
    /// 在工具调用循环中按名称解析。
    ///
    /// 与 `self.tools`（发送给 LLM 的 JSON Schema）不同，此字段携带实际的
    /// `Arc<dyn ITool>` 实例用于调用。
    #[serde(skip)]
    pub provider_tools: Vec<Arc<dyn ITool>>,
    /// 从 `AgentRunOptions` 传播过来的工具审批响应
    /// 用于在审批暂停后继续执行
    #[serde(skip)]
    pub tool_approval_responses: Vec<ToolApprovalResponse>,
    /// 从 `AgentRunOptions` 传播过来的取消标志
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
    /// 创建默认运行选项
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置单次调用的最大 Token 数
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    /// 设置单次调用的温度参数
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    /// 添加额外的请求体字段（仅本次调用生效）
    pub fn with_extra_body(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.extra_body.insert(key.into(), value);
        self
    }

    /// 设置单次调用的工具定义
    pub fn with_tools(mut self, tools: Vec<serde_json::Value>) -> Self {
        self.tools = tools;
        self
    }
}

/// 聊天客户端接口，遵循 MAF 的 ChatClient 抽象。
///
/// 对 LLM 提供商 API 的轻量封装。
/// 仅支持流式输出。
#[async_trait]
pub trait IChatClient: Send + Sync + Any {
    /// 执行聊天补全，产生更新增量的流式响应
    ///
    /// `options` 允许单次调用覆盖默认参数（温度、额外请求体等），
    /// 而不修改客户端的持久配置。传递 `Default::default()` 使用默认行为。
    async fn run(
        &self,
        messages: &[ChatMessage],
        options: ChatClientRunOptions,
    ) -> Result<BoxStream<'static, Result<AgentResponseUpdate>>>;

    /// 返回此客户端使用的模型标识符
    fn model_id(&self) -> &str;

    /// 返回模型元数据，描述能力边界（上下文窗口、最大输出等）
    ///
    /// 压缩策略和框架使用此信息强制 Token 限制。
    /// 当模型边界未知时返回 `None`（默认行为）。
    /// 具体实现应重写此方法。
    fn model_metadata(&self) -> Option<&ModelMetadata> {
        None
    }

    /// 返回装饰器链中的内部客户端，叶子（API）客户端返回 `None`
    ///
    /// 上下文提供器（如 KnowledgeBundle）使用此方法解包装饰器层，
    /// 以访问原始 API 客户端。装饰器如 `FunctionInvokingChatClient`
    /// 重写此方法；叶子客户端保持默认的 `None`。
    fn inner_client(&self) -> Option<&Arc<dyn IChatClient>> {
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
    /// 创建装饰器，包裹给定的内部客户端
    pub fn new(inner: Arc<dyn IChatClient>) -> Self {
        Self { inner }
    }

    /// 获取被包裹的内部客户端的引用
    pub fn inner(&self) -> &Arc<dyn IChatClient> {
        &self.inner
    }
}

#[async_trait]
impl IChatClient for DelegatingChatClient {
    /// 透传调用 — 委托给内部客户端执行聊天补全
    async fn run(
        &self,
        messages: &[ChatMessage],
        options: ChatClientRunOptions,
    ) -> Result<BoxStream<'static, Result<AgentResponseUpdate>>> {
        self.inner.run(messages, options).await
    }

    /// 透传获取模型标识符
    fn model_id(&self) -> &str {
        self.inner.model_id()
    }

    /// 透传获取模型元数据
    fn model_metadata(&self) -> Option<&ModelMetadata> {
        self.inner.model_metadata()
    }

    /// 返回被包裹的内部客户端（非叶子节点）
    fn inner_client(&self) -> Option<&Arc<dyn IChatClient>> {
        Some(&self.inner)
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
    /// 创建空的管道构建器
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
    /// 默认实现：创建一个空管道构建器
    fn default() -> Self {
        Self::new()
    }
}
