use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::{
    ApprovalRequiredTool, IContextProvider, ITool, ProviderState, Result, ScopePolicy,
    WorkspaceScope,
};
use serde::{Deserialize, Serialize};

/// 工作区管理的主入口。
///
/// 对标 `AgentSkillsProvider`：提供工具注入 + 指令注入的 `IContextProvider`。
/// Scope 通过构造函数注入（编译期保证必须提供）。
///
/// 使用方式：
/// ```ignore
/// let scope = WorkspaceScope::new("/project", "my-project")
///     .with_policy(ScopePolicy::ApproveOutside);
///
/// let provider = WorkspaceContextProvider::new(Arc::new(scope))
///     .add_tool(ReadFile::default())
///     .add_tool(WriteFile::default())
///     .add_tool(RunCommand::default());
/// ```
pub struct WorkspaceContextProvider {
    scope: Arc<WorkspaceScope>,
    tools: Vec<Arc<dyn ITool>>,
}

impl WorkspaceContextProvider {
    /// 构造函数注入——scope 必须提供。
    pub fn new(scope: Arc<WorkspaceScope>) -> Self {
        Self {
            scope,
            tools: Vec::new(),
        }
    }

    /// 添加需要工作区管理的工具（构造器模式，消费 self）。
    ///
    /// 内部处理：
    /// 1. 若工具实现 `IScopeTool` → 自动调用 `create_scoped()` 注入 scope
    /// 2. 若 `ScopePolicy` 为 `ApproveOutside` → 包裹 `ApprovalRequiredTool`
    pub fn add_tool(mut self, tool: impl ITool + 'static) -> Self {
        let mut tool: Arc<dyn ITool> = Arc::new(tool);

        // Step 1: scope 注入（检测 IScopeTool）
        if let Some(scoped) = try_inject_scope(&tool, Arc::clone(&self.scope)) {
            tool = scoped;
        }

        // Step 2: 审批包裹（按策略）
        if self.scope.policy == ScopePolicy::ApproveOutside {
            tool = Arc::new(ApprovalRequiredTool::new(tool));
        }

        self.tools.push(tool);
        self
    }

    /// 添加已解析为 `Arc<dyn ITool>` 的工具（不消费 self，用于声明式构建路径）。
    ///
    /// 与 `add_tool()` 执行相同的两阶段处理（scope 注入 + 审批包裹），
    /// 但接受 `Arc<dyn ITool>` 并通过 `&mut self` 修改，适合在工具解析完成后
    /// 将工具路由到工作区管理的场景。
    pub fn add_tool_arc(&mut self, tool: Arc<dyn ITool>) {
        let mut tool = tool;

        // Step 1: scope 注入（检测 IScopeTool）
        if let Some(scoped) = try_inject_scope(&tool, Arc::clone(&self.scope)) {
            tool = scoped;
        }

        // Step 2: 审批包裹（按策略）
        if self.scope.policy == ScopePolicy::ApproveOutside {
            tool = Arc::new(ApprovalRequiredTool::new(tool));
        }

        self.tools.push(tool);
    }

    fn build_instructions(&self) -> String {
        let policy_desc = match self.scope.policy {
            ScopePolicy::AllowAll => "无限制（所有路径均可访问）",
            ScopePolicy::ApproveOutside => "跨范围审批（工作区外的操作需用户审批后方可执行）",
            ScopePolicy::DenyOutside => "禁止越界（工作区外操作直接拒绝）",
        };
        format!(
            "## 工作区\n\
             名称: {name}\n\
             根路径: {root}\n\
             越界策略: {policy}\n\n\
             - 相对路径在工作区内解析\n\
             - 绝对路径若在工作区外，工具返回中 scope 字段会标明 outside_workspace\n\
             - 每个工具返回均包含 scope 字段以标明操作范围",
            name = self.scope.name,
            root = self.scope.root.display(),
            policy = policy_desc,
        )
    }
}

/// 通过 `ITool::as_scope_tool()` 检测 `IScopeTool` 并注入 scope。
///
/// 统一检测机制——无需维护硬编码的类型列表。任何实现 `IScopeTool`
/// 并覆写 `as_scope_tool()` 的工具都会被自动检测。
fn try_inject_scope(
    tool: &Arc<dyn ITool>,
    scope: Arc<WorkspaceScope>,
) -> Option<Arc<dyn ITool>> {
    tool.as_scope_tool().map(|scope_tool| {
        scope_tool.create_scoped(scope)
    })
}

#[async_trait]
impl IContextProvider for WorkspaceContextProvider {
    fn name(&self) -> &str {
        "WorkspaceContextProvider"
    }

    fn kind(&self) -> &str {
        "workspace"
    }

    async fn enrich_instructions(&self, ctx: &rust_agent_core::ProviderContext<'_>) -> Result<Option<String>> {
        // 会话级持久化（仅首次，用于审计/调试）
        let state = ProviderState::<WorkspaceState>::new("WorkspaceContextProvider");
        let ws = state.get_or_init(ctx.session);
        if ws.scope_name.is_empty() {
            let _ = state.save(
                ctx.session,
                &WorkspaceState {
                    scope_name: self.scope.name.clone(),
                    scope_root: self.scope.root.to_string_lossy().to_string(),
                    policy: format!("{:?}", self.scope.policy),
                },
            );
        }
        Ok(Some(self.build_instructions()))
    }

    async fn enrich_tools(&self, _ctx: &rust_agent_core::ProviderContext<'_>) -> Result<Vec<Arc<dyn ITool>>> {
        Ok(self.tools.clone())
    }

    // on_invoked 不再覆写——使用默认空实现，消除样板代码
}

#[derive(Default, Serialize, Deserialize)]
struct WorkspaceState {
    scope_name: String,
    scope_root: String,
    policy: String,
}
