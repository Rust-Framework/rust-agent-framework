use rust_agent_workflow::graph::WorkflowGraph;

use crate::compiler::{compile_workflow, prewarm_workflow_tools};
use crate::compiler::registry::CompileRegistry;
use crate::definition::{AgentDefinition, AgentKindData};
use crate::error::DeclError;
use crate::workflow_decl::WorkflowAgentData;

/// 将 `WorkflowAgentData` 解析为可执行的 `WorkflowGraph`。
pub struct WorkflowResolver<'a> {
    registry: &'a mut CompileRegistry,
}

impl<'a> WorkflowResolver<'a> {
    pub fn new(registry: &'a mut CompileRegistry) -> Self {
        Self { registry }
    }

    pub async fn resolve(&mut self, data: &WorkflowAgentData) -> crate::Result<WorkflowGraph> {
        prewarm_workflow_tools(&data.trigger.actions, self.registry).await?;
        compile_workflow(data, self.registry)
    }
}

/// 将工作流 Agent 定义解析为图。
pub async fn resolve_workflow(def: &AgentDefinition) -> crate::Result<WorkflowGraph> {
    match &def.kind_data {
        AgentKindData::Workflow(data) => {
            let mut registry = CompileRegistry::new();
            let mut workflow_resolver = WorkflowResolver::new(&mut registry);
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
