use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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

    /// 工具分类——与 ToolDecl 的 kind 标签对应。
    ///
    /// 默认返回 `ToolKind::Unknown`，内置工具和宏生成的结构体应覆写此方法。
    fn kind(&self) -> crate::ToolKind {
        crate::ToolKind::Unknown
    }
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
    fn kind(&self) -> crate::ToolKind {
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

/// ToolRegistry — 管理工具注册和查找，遵循 MAF 模式。
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn ITool>>,
}

impl ToolRegistry {
    pub fn new() -> Self { Self { tools: HashMap::new() } }

    pub fn register(&mut self, tool: impl ITool + 'static) {
        self.tools.insert(tool.name().to_string(), Arc::new(tool));
    }

    pub fn register_arc(&mut self, tool: Arc<dyn ITool>) {
        self.tools.insert(tool.name().to_string(), tool);
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
