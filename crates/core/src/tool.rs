use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::Result;

/// 工具接口，遵循 MAF 的工具抽象。
#[async_trait]
pub trait ITool: Send + Sync {
    /// 获取工具名称
    fn name(&self) -> &str;
    /// 获取工具描述
    fn description(&self) -> &str;
    /// 获取工具参数 JSON Schema
    fn parameters(&self) -> serde_json::Value;
    /// 执行工具并返回执行结果
    async fn execute(&self, arguments: serde_json::Value) -> Result<String>;

    /// 运行时标记：返回 `true` 表示需要人工审批才能执行
    ///
    /// 默认返回 `false`（自动执行）。仅 [`ApprovalRequiredTool`] 重写为返回 `true`。
    /// 由 `FunctionInvokingChatClient` 在执行前检查。
    fn requires_approval(&self) -> bool {
        false
    }
}

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
    /// 创建一个需要审批的包装工具
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
    /// 透传获取被包装工具的名称
    fn name(&self) -> &str {
        self.inner.name()
    }
    /// 透传获取被包装工具的描述
    fn description(&self) -> &str {
        self.inner.description()
    }
    /// 透传获取被包装工具的参数 Schema
    fn parameters(&self) -> serde_json::Value {
        self.inner.parameters()
    }
    /// 透传执行被包装的工具
    async fn execute(&self, arguments: serde_json::Value) -> Result<String> {
        self.inner.execute(arguments).await
    }
    /// 始终返回 `true`，标记此工具需要审批
    fn requires_approval(&self) -> bool {
        true
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
    /// 创建空的工具注册表
    pub fn new() -> Self { Self { tools: HashMap::new() } }

    /// 注册一个工具（按名称索引）
    pub fn register(&mut self, tool: impl ITool + 'static) {
        self.tools.insert(tool.name().to_string(), Arc::new(tool));
    }

    /// 注册一个已共享（`Arc`）的工具
    pub fn register_arc(&mut self, tool: Arc<dyn ITool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    /// 按名称查找工具
    pub fn get(&self, name: &str) -> Option<&Arc<dyn ITool>> {
        self.tools.get(name)
    }

    /// 获取所有已注册的工具列表
    pub fn list(&self) -> Vec<&Arc<dyn ITool>> {
        self.tools.values().collect()
    }

    /// 获取已注册工具的数量
    pub fn len(&self) -> usize { self.tools.len() }
    /// 检查是否没有注册任何工具
    pub fn is_empty(&self) -> bool { self.tools.is_empty() }
}

impl Default for ToolRegistry {
    /// 创建空的工具注册表
    fn default() -> Self { Self::new() }
}
