use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::Result;

// ── AsAny ────────────────────────────────────────────────────────────────

/// 为 trait object 提供运行时下转型能力。
///
/// `ITool` 继承此 trait，使 `Arc<dyn ITool>` 可通过 `as_any()` 下转
/// 到具体类型。`WorkspaceContextProvider` 用此检测 `IScopeTool` 实现。
pub trait AsAny {
    fn as_any(&self) -> &dyn std::any::Any;
}

/// 为所有 `'static` 类型自动实现 `AsAny`。
impl<T: 'static> AsAny for T {
    fn as_any(&self) -> &dyn std::any::Any { self }
}

// ── ToolResult ───────────────────────────────────────────────────────────

/// 工具执行结果——框架级统一返回类型。
///
/// 所有 `ITool::execute()` 返回此结构体：
/// - 成功：`ToolResult::success(data)`
/// - 预期错误（如文件不存在）：`ToolResult::error("File not found")`
/// - 框架错误（如参数反序列化失败）：`Result::Err(AgentError)`
///
/// 框架层（`FunctionInvokingChatClient`）统一将 `ToolResult` 序列化
/// 为 JSON 字符串注入 LLM 对话。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ToolResult {
    /// 创建成功结果。
    pub fn success(data: impl Serialize) -> Self {
        Self {
            ok: true,
            data: Some(serde_json::to_value(data).unwrap_or_default()),
            error: None,
        }
    }

    /// 创建工具级错误（非框架异常）。
    pub fn error(msg: impl Into<String>) -> Self {
        Self { ok: false, data: None, error: Some(msg.into()) }
    }

    /// 创建带结构化错误数据的错误结果（如校验失败的字段详情）。
    pub fn error_with_data(msg: impl Into<String>, data: impl Serialize) -> Self {
        Self {
            ok: false,
            data: Some(serde_json::to_value(data).unwrap_or_default()),
            error: Some(msg.into()),
        }
    }
}

// ── ITool ────────────────────────────────────────────────────────────────

/// 工具接口，遵循 MAF 的工具抽象。
#[async_trait]
pub trait ITool: AsAny + Send + Sync {
    /// 获取工具名称
    fn name(&self) -> &str;
    /// 获取工具描述
    fn description(&self) -> &str;
    /// 获取工具参数 JSON Schema
    fn parameters(&self) -> serde_json::Value;

    /// 执行业务逻辑。
    ///
    /// - `Ok(ToolResult)`：工具执行完成（含成功或工具级预期错误）
    /// - `Err(AgentError)`：框架级错误（参数反序列化失败等）
    ///
    /// 框架层负责将 `ToolResult` 序列化为 JSON 字符串注入 LLM 对话。
    async fn execute(&self, arguments: serde_json::Value) -> Result<ToolResult>;

    /// 运行时标记：返回 `true` 表示需要人工审批才能执行
    ///
    /// 默认返回 `false`（自动执行）。仅 [`ApprovalRequiredTool`] 重写为返回 `true`。
    /// 由 `FunctionInvokingChatClient` 在执行前检查。
    fn requires_approval(&self) -> bool {
        false
    }

    /// 工具分类——开放字符串，由实现者自行定义。
    ///
    /// 内置约定值：`"function"`、`"file"`、`"shell"`、`"code"`、`"mcp"`、
    /// `"web"`、`"skills"`、`"custom"`、`"openapi"`。
    /// 插件可返回任意自定义字符串，框架不做封闭枚举约束。
    /// 默认返回 `"unknown"`。
    fn kind(&self) -> &str {
        "unknown"
    }

    /// 尝试将此工具转换为 `IScopeTool` 引用。
    ///
    /// 默认返回 `None`。实现 `IScopeTool` 的工具应覆写此方法返回 `Some(self)`，
    /// 使 `WorkspaceContextProvider` 能统一检测 scope-aware 工具，无需维护
    /// 硬编码的类型列表。
    ///
    /// 对标 MAF 原则 3：Trait 即契约，组合即架构。此方法消除了
    /// `try_inject_scope()` 和 `partition_scope_tools()` 中的 11 类型 downcast 列表。
    fn as_scope_tool(&self) -> Option<&dyn crate::IScopeTool> {
        None
    }
}

// ── Callable trait ──────────────────────────────────────────────────────

/// 类型安全的工具调用路径——对标 MAF 的类型化工具契约。
///
/// `#[tool]` 宏生成的结构体同时实现 `ITool`（LLM 路径，走 JSON）和
/// `Callable<Args, Ret>`（Rust 路径，类型安全）。外部调用者可通过
/// Rust 代码类型安全地构造工具调用，避免 JSON 序列化往返开销。
///
/// # 零成本抽象
///
/// `Callable::call()` 直接调用 `self.call(args)`，无装箱/拆箱开销。
/// 这与 MAF 的反射调用形成对比——Rust 在编译期生成直接调用。
///
/// # 示例
///
/// ```ignore
/// // Rust 路径：类型安全，零序列化开销
/// let tool = ReadFile;
/// let args = ReadFileArgs { path: "test.txt".into() };
/// let result = tool.call(args).await?;
///
/// // LLM 路径：通过 ITool::execute，走 JSON
/// let json_args = serde_json::to_value(&args)?;
/// let result = tool.execute(json_args).await?;
/// ```
#[async_trait]
pub trait Callable<Args, Ret> {
    /// 类型安全的调用方法——直接调用，无 JSON 序列化往返。
    async fn call(&self, args: Args) -> Ret;
}

// ── ApprovalRequiredTool ─────────────────────────────────────────────────

/// 包装任意 [`ITool`]，标记为需要人工审批才能执行。
///
/// 对应 MAF 的 `ApprovalRequiredAIFunction`。
///
/// `FunctionInvokingChatClient` 在运行时检查 `requires_approval()`，
/// 当返回 `true` 时，发出 [`ToolApprovalRequest`](crate::AgentResponseUpdate::ToolApprovalRequest)
/// 事件而非立即执行。
///
/// # 使用方式
///
/// ```ignore
/// // Agent A：自动执行
/// builder.with_tool(RunCommand);
///
/// // Agent B：需要审批（生产环境）
/// builder.with_tool(ApprovalRequiredTool::new(Arc::new(RunCommand)));
/// ```
#[derive(Clone)]
pub struct ApprovalRequiredTool {
    pub inner: Arc<dyn ITool>,
}

impl ApprovalRequiredTool {
    pub fn new(tool: Arc<dyn ITool>) -> Self {
        Self { inner: tool }
    }
}

impl std::fmt::Debug for ApprovalRequiredTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApprovalRequiredTool")
            .field("inner", &format_args!("{}", self.inner.name()))
            .finish()
    }
}

#[async_trait]
impl ITool for ApprovalRequiredTool {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn description(&self) -> &str {
        self.inner.description()
    }
    fn parameters(&self) -> serde_json::Value {
        self.inner.parameters()
    }
    async fn execute(&self, arguments: serde_json::Value) -> Result<ToolResult> {
        self.inner.execute(arguments).await
    }
    fn requires_approval(&self) -> bool {
        true
    }
    fn kind(&self) -> &str {
        self.inner.kind()
    }
}

/// 调用者对 [`ToolApprovalRequest`](crate::AgentResponseUpdate::ToolApprovalRequest) 的响应。
///
/// 对应 MAF 的 `FunctionApprovalResponseContent`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolApprovalResponse {
    /// 匹配对应 `ToolApprovalRequest` 中的 `call_id`
    pub call_id: String,
    /// `true` = 批准执行，`false` = 拒绝
    pub approved: bool,
    /// 拒绝原因（可选，会反馈给 LLM）
    pub reason: Option<String>,
}

// ── Plugin ───────────────────────────────────────────────────────────────

/// 插件——封装一组工具为一个可注册单元，对应 MAF 的 KernelPlugin。
///
/// 插件提供命名空间隔离：[`ToolRegistry::register_plugin`] 会将每个工具名
/// 自动限定为 `"{namespace}/{tool_name}"`，使不同插件的同名工具可共存。
///
/// # 示例
///
/// ```ignore
/// pub struct FilePlugin;
///
/// impl Plugin for FilePlugin {
///     fn namespace(&self) -> &str { "file" }
///     fn tools(&self) -> Vec<Arc<dyn ITool>> {
///         vec![Arc::new(ReadFile::default()), Arc::new(WriteFile::default())]
///     }
/// }
///
/// registry.register_plugin(&FilePlugin)?;
/// // "file/read_file" 和 "file/write_file" 可用，与 "db/read_file" 不冲突
/// ```
pub trait Plugin: Send + Sync {
    /// 插件命名空间（如 `"file"`、`"db"`、`"rag"`）。
    fn namespace(&self) -> &str;

    /// 插件提供的所有工具。
    fn tools(&self) -> Vec<Arc<dyn ITool>>;
}

/// 命名空间包装器——将工具名限定为 `"{ns}/{original_name}"`。
///
/// 由 [`ToolRegistry::register_plugin`] 内部使用，用户无需直接构造。
pub struct NamespacedTool {
    full_name: String,
    inner: Arc<dyn ITool>,
}

impl NamespacedTool {
    pub fn new(namespace: &str, tool: Arc<dyn ITool>) -> Self {
        Self {
            full_name: format!("{}/{}", namespace, tool.name()),
            inner: tool,
        }
    }
}

impl std::fmt::Debug for NamespacedTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NamespacedTool")
            .field("full_name", &self.full_name)
            .finish()
    }
}

#[async_trait]
impl ITool for NamespacedTool {
    fn name(&self) -> &str { &self.full_name }
    fn description(&self) -> &str { self.inner.description() }
    fn parameters(&self) -> serde_json::Value { self.inner.parameters() }
    async fn execute(&self, arguments: serde_json::Value) -> Result<ToolResult> {
        self.inner.execute(arguments).await
    }
    fn requires_approval(&self) -> bool { self.inner.requires_approval() }
    fn kind(&self) -> &str { self.inner.kind() }
}

// ── ToolRegistryError ────────────────────────────────────────────────────

/// 工具注册错误。
#[derive(Debug, Clone, thiserror::Error)]
pub enum ToolRegistryError {
    /// 工具名冲突——已有同名工具注册，拒绝静默覆盖。
    #[error("tool name conflict: '{name}' is already registered")]
    Conflict { name: String },
}

// ── ToolRegistry ─────────────────────────────────────────────────────────

/// 工具注册表——管理工具注册、查找和插件命名空间，对应 MAF 的 ToolRegistry。
///
/// 使用 `BTreeMap` 存储以支持 `"/"` 层级路径排序和命名空间前缀扫描。
/// 注册时检测名称冲突，返回 `Err(ToolRegistryError::Conflict)` 而非静默覆盖。
pub struct ToolRegistry {
    tools: BTreeMap<String, Arc<dyn ITool>>,
}

impl ToolRegistry {
    pub fn new() -> Self { Self { tools: BTreeMap::new() } }

    /// 注册单个工具。名称冲突时返回错误，而非静默覆盖。
    pub fn register(&mut self, tool: impl ITool + 'static) -> std::result::Result<(), ToolRegistryError> {
        self.register_arc(Arc::new(tool))
    }

    /// 注册 `Arc<dyn ITool>`。名称冲突时返回错误，而非静默覆盖。
    pub fn register_arc(&mut self, tool: Arc<dyn ITool>) -> std::result::Result<(), ToolRegistryError> {
        let name = tool.name().to_string();
        if self.tools.contains_key(&name) {
            return Err(ToolRegistryError::Conflict { name });
        }
        self.tools.insert(name, tool);
        Ok(())
    }

    /// 原子化注册整个插件。每个工具名自动限定为 `"{namespace}/{tool_name}"`。
    ///
    /// 先检查所有工具名是否冲突，全部通过后才插入——保证原子性。
    /// 对应 MAF 的 `KernelPlugin` 批量注册。
    pub fn register_plugin(&mut self, plugin: &dyn Plugin) -> std::result::Result<(), ToolRegistryError> {
        let ns = plugin.namespace();
        let namespaced: Vec<Arc<dyn ITool>> = plugin
            .tools()
            .into_iter()
            .map(|t| Arc::new(NamespacedTool::new(ns, t)) as Arc<dyn ITool>)
            .collect();

        // 冲突预检——全部通过后才插入
        for tool in &namespaced {
            let name = tool.name();
            if self.tools.contains_key(name) {
                return Err(ToolRegistryError::Conflict { name: name.to_string() });
            }
        }

        // 原子插入
        for tool in namespaced {
            let name = tool.name().to_string();
            self.tools.insert(name, tool);
        }
        Ok(())
    }

    /// 注销指定命名空间下的所有工具，返回移除的数量。
    ///
    /// 支持运行时动态卸载插件——对应 MAF 的插件生命周期管理。
    pub fn unregister_namespace(&mut self, namespace: &str) -> usize {
        let prefix = format!("{}/", namespace);
        let keys: Vec<String> = self
            .tools
            .keys()
            .filter(|k| k.starts_with(&prefix))
            .cloned()
            .collect();
        let count = keys.len();
        for k in keys {
            self.tools.remove(&k);
        }
        count
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn ITool>> {
        self.tools.get(name)
    }

    pub fn list(&self) -> Vec<&Arc<dyn ITool>> {
        self.tools.values().collect()
    }

    pub fn len(&self) -> usize { self.tools.len() }
    pub fn is_empty(&self) -> bool { self.tools.is_empty() }
}

impl Default for ToolRegistry {
    fn default() -> Self { Self::new() }
}
