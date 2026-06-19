use rust_agent_workflow::graph::WorkflowGraph;

use crate::compiler::compile_workflow;
use crate::definition::{AgentDefinition, AgentKindData};
use crate::error::DeclError;
#[allow(deprecated)]
use crate::resolver::agent_resolver::AgentResolver;
use crate::workflow_decl::WorkflowAgentData;

/// 将 `WorkflowAgentData` 解析为可执行的 `WorkflowGraph`。
///
/// 此解析器将 MAF 动作列表 DSL 编译为可由工作流引擎执行的图。
///
/// 全量支持 23 种 ActionDecl 动作类型：
/// - 变量管理：SetVariable / SetMultipleVariables / SetTextVariable / ResetVariable / ClearAllVariables / ParseValue / EditTableV2
/// - 控制流：If / ConditionGroup / Foreach / GotoAction / BreakLoop / ContinueLoop
/// - AI 与输出：InvokeAgent / SendActivity / InvokeFunctionTool
/// - 人机交互：Question / RequestExternalInput
/// - HTTP/MCP：HttpRequestAction / InvokeMcpTool
/// - 终端与对话：EndWorkflow / EndConversation / CreateConversation / AddConversationMessage
///
/// 编译架构：ActionDecl → CompileNode(IR) → WorkflowGraph
#[allow(deprecated)]
pub struct WorkflowResolver<'a> {
    agent_resolver: &'a mut AgentResolver,
}

impl<'a> WorkflowResolver<'a> {
    /// 使用给定的 Agent 解析器创建新的工作流解析器。
    #[allow(deprecated)]
    pub fn new(agent_resolver: &'a mut AgentResolver) -> Self {
        Self { agent_resolver }
    }

    /// 将 `WorkflowAgentData` 解析为 `WorkflowGraph`。
    ///
    /// 使用全新的双层编译引擎（参见 `crate::compiler`）。
    pub async fn resolve(&mut self, data: &WorkflowAgentData) -> crate::Result<WorkflowGraph> {
        compile_workflow(data, self.agent_resolver)
    }
}

/// 将工作流 Agent 定义解析为图。
#[allow(deprecated)]
pub async fn resolve_workflow(def: &AgentDefinition) -> crate::Result<WorkflowGraph> {
    match &def.kind_data {
        AgentKindData::Workflow(data) => {
            let mut agent_resolver = AgentResolver::new();
            let mut workflow_resolver = WorkflowResolver::new(&mut agent_resolver);
            workflow_resolver.resolve(data).await
        }
        _ => Err(DeclError::Validation(format!(
            "Expected workflow agent, got non-workflow definition '{}'",
            def.name
        ))),
    }
}

/// 快速一行程序：从文件解析 `WorkflowDecl` 并构建图。
pub async fn quick_workflow(path: &str) -> crate::Result<WorkflowGraph> {
    let doc = crate::document::AgentDocument::from_json_file(path)?;
    let def = doc.inner_definition();
    resolve_workflow(def).await
}
