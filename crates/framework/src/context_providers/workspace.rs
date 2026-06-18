use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::{
    AgentResponse, AgentRunOptions, ApprovalRequiredTool, ChatMessage, ContextInjection,
    IAgent, IContextProvider, IScopeTool, ISession, ITool, ProviderState, Result, ScopePolicy,
    WorkspaceScope,
};
use serde::{Deserialize, Serialize};

use crate::tools::{
    EditFile, FindFiles, InspectFile, ListFiles, MakeDirectory, MoveFile, ReadFile, RemovePath,
    RunCommand, SearchFile, WriteFile,
};

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
///     .add_tool(ReadFile { scope: None })
///     .add_tool(WriteFile { scope: None })
///     .add_tool(RunCommand { scope: None, timeout_secs: None });
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

    /// 添加需要工作区管理的工具。
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

/// 通过 `AsAny` 下转型检测 `IScopeTool` 并注入 scope。
fn try_inject_scope(
    tool: &Arc<dyn ITool>,
    scope: Arc<WorkspaceScope>,
) -> Option<Arc<dyn ITool>> {
    use rust_agent_core::AsAny;
    let any = tool.as_any();

    // 为每个已知工具类型尝试下转型并调用 create_scoped
    if any.downcast_ref::<ReadFile>().is_some() {
        let dummy = ReadFile { scope: None };
        return Some(dummy.create_scoped(scope));
    }
    if any.downcast_ref::<WriteFile>().is_some() {
        let dummy = WriteFile { scope: None };
        return Some(dummy.create_scoped(scope));
    }
    if any.downcast_ref::<EditFile>().is_some() {
        let dummy = EditFile { scope: None };
        return Some(dummy.create_scoped(scope));
    }
    if any.downcast_ref::<ListFiles>().is_some() {
        let dummy = ListFiles { scope: None };
        return Some(dummy.create_scoped(scope));
    }
    if any.downcast_ref::<InspectFile>().is_some() {
        let dummy = InspectFile { scope: None };
        return Some(dummy.create_scoped(scope));
    }
    if any.downcast_ref::<MakeDirectory>().is_some() {
        let dummy = MakeDirectory { scope: None };
        return Some(dummy.create_scoped(scope));
    }
    if any.downcast_ref::<RemovePath>().is_some() {
        let dummy = RemovePath { scope: None };
        return Some(dummy.create_scoped(scope));
    }
    if any.downcast_ref::<MoveFile>().is_some() {
        let dummy = MoveFile { scope: None };
        return Some(dummy.create_scoped(scope));
    }
    if any.downcast_ref::<FindFiles>().is_some() {
        let dummy = FindFiles { scope: None };
        return Some(dummy.create_scoped(scope));
    }
    if any.downcast_ref::<SearchFile>().is_some() {
        let dummy = SearchFile { scope: None };
        return Some(dummy.create_scoped(scope));
    }
    if any.downcast_ref::<RunCommand>().is_some() {
        let dummy = RunCommand {
            scope: None,
            timeout_secs: None,
        };
        return Some(dummy.create_scoped(scope));
    }
    None
}

#[async_trait]
impl IContextProvider for WorkspaceContextProvider {
    fn name(&self) -> &str {
        "WorkspaceContextProvider"
    }

    async fn on_invoking(
        &self,
        _agent: &dyn IAgent,
        session: &dyn ISession,
        _messages: &[ChatMessage],
        _options: &AgentRunOptions,
    ) -> Result<ContextInjection> {
        // 会话级持久化（仅首次，用于审计/调试）
        let state = ProviderState::<WorkspaceState>::new("WorkspaceContextProvider");
        let ws = state.get_or_init(session);
        if ws.scope_name.is_empty() {
            let _ = state.save(
                session,
                &WorkspaceState {
                    scope_name: self.scope.name.clone(),
                    scope_root: self.scope.root.to_string_lossy().to_string(),
                    policy: format!("{:?}", self.scope.policy),
                },
            );
        }

        Ok(ContextInjection {
            instructions: Some(self.build_instructions()),
            tools: self.tools.clone(),
            ..Default::default()
        })
    }

    async fn on_invoked(
        &self,
        _agent: &dyn IAgent,
        _session: &dyn ISession,
        _request_messages: &[ChatMessage],
        _response: Option<&AgentResponse>,
        _error: Option<&rust_agent_core::AgentError>,
    ) -> Result<()> {
        Ok(())
    }
}

#[derive(Default, Serialize, Deserialize)]
struct WorkspaceState {
    scope_name: String,
    scope_root: String,
    policy: String,
}
