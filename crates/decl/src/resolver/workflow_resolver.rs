use std::sync::Arc;

use rust_agent_workflow::builder::WorkflowBuilder;
use rust_agent_workflow::executor::AgentExecutor;
use rust_agent_workflow::graph::WorkflowGraph;
use rust_agent_workflow::graph::port::RequestPort;
use rust_agent_workflow::executor::base::TypeTag;

use crate::actions::ActionDecl;
use crate::definition::{AgentDefinition, AgentKindData};
use crate::error::DeclError;
use crate::resolver::agent_resolver::AgentResolver;
use crate::workflow_decl::WorkflowAgentData;

/// 将 `WorkflowAgentData` 解析为可执行的 `WorkflowGraph`。
///
/// 此解析器将 MAF 动作列表 DSL 编译为可由工作流引擎执行的图。
///
/// 当前支持动作的子集：
/// - `InvokeAgent` → 创建 AgentExecutor 节点
/// - `SendActivity` → 创建输出发射器
/// - `SetVariable` → 创建状态变更节点
/// 未来阶段将添加对 If、Foreach、ConditionGroup 等的完整支持。
pub struct WorkflowResolver<'a> {
    agent_resolver: &'a mut AgentResolver,
}

impl<'a> WorkflowResolver<'a> {
    /// 使用给定的 Agent 解析器创建新的工作流解析器。
    pub fn new(agent_resolver: &'a mut AgentResolver) -> Self {
        Self { agent_resolver }
    }

    /// 将 `WorkflowAgentData` 解析为 `WorkflowGraph`。
    pub async fn resolve(&mut self, data: &WorkflowAgentData) -> crate::Result<WorkflowGraph> {
        let mut builder = WorkflowBuilder::new();
        let mut prev_node_id: Option<String> = None;

        for action in &data.trigger.actions {
            match action {
                ActionDecl::InvokeAgent {
                    id,
                    agent,
                    ..
                } => {
                    let node_id = id
                        .clone()
                        .unwrap_or_else(|| format!("node_{}_{}", agent.name, uuid::Uuid::new_v4().to_string().split('-').next().unwrap_or("0")));

                    let agent_instance = self
                        .agent_resolver
                        .get_agent(&agent.name)
                        .ok_or_else(|| {
                            DeclError::Missing(format!(
                                "Agent '{}' not found in registry (referenced by workflow action)",
                                agent.name
                            ))
                        })?;

                    let executor = AgentExecutor::new(&node_id, agent_instance);
                    builder = builder.add_node(node_id.clone(), Arc::new(executor));

                    // Wire sequential edges
                    if let Some(ref prev) = prev_node_id {
                        builder = builder.add_edge(prev.as_str(), &node_id);
                    }
                    prev_node_id = Some(node_id);
                }

                ActionDecl::SendActivity { id, .. } => {
                    // SendActivity creates an output port node
                    let node_id = id
                        .clone()
                        .unwrap_or_else(|| format!("output_{}", uuid::Uuid::new_v4().to_string().split('-').next().unwrap_or("0")));

                    let request_port = RequestPort::new(
                        &node_id,
                        TypeTag::new("json"),
                        TypeTag::new("json"),
                        "",
                    );
                    builder = builder.add_port(request_port);
                }

                ActionDecl::SetVariable { id, variable, value, .. } => {
                    // SetVariable creates a state mutation node
                    // For now, we treat it as a no-op pass-through
                    let _node_id = id
                        .clone()
                        .unwrap_or_else(|| format!("var_{}_{}", variable, uuid::Uuid::new_v4().to_string().split('-').next().unwrap_or("0")));
                    // Track variable setting for the state store
                    // (full implementation in a future phase)
                    let _ = value; // Used when state mutation is fully implemented
                }

                // Control flow actions — not yet implemented in graph compiler
                ActionDecl::If { .. }
                | ActionDecl::ConditionGroup { .. }
                | ActionDecl::Foreach { .. }
                | ActionDecl::GotoAction { .. }
                | ActionDecl::Question { .. }
                | ActionDecl::RequestExternalInput { .. } => {
                    return Err(DeclError::Unsupported(format!(
                        "Action kind '{}' is not yet supported in the workflow graph compiler",
                        action.kind_str()
                    )));
                }

                _ => {
                    // BreakLoop, ContinueLoop, EndWorkflow, EndConversation,
                    // CreateConversation, AddConversationMessage, etc.
                    // These are terminal/no-op actions that don't produce nodes.
                }
            }
        }

        // Set start node
        if let Some(first_node) = prev_node_id.take() {
            builder = builder.set_start(first_node);
        } else {
            // Find the first node from the builder
            // In practice, set_start will fail if no node was added
            return Err(DeclError::Validation(
                "Workflow must have at least one InvokeAgent action".into(),
            ));
        }

        let graph = builder.build().map_err(|e| {
            DeclError::Resolution(format!("Failed to build workflow graph: {}", e))
        })?;

        Ok(graph)
    }
}

/// 将工作流 Agent 定义解析为图。
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
