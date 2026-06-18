//! Declaration Workflow Compiler
//!
//! 将 MAF-aligned `ActionDecl` 序列编译为可执行的 `WorkflowGraph`。
//!
//! ## 架构
//!
//! ```ignore
//! ActionDecl[] → CompileNode (IR) → WorkflowGraph
//! ```
//!
//! - `ir.rs` — `CompileNode` 中间表示 + `ExecutorKind` 执行器种类
//! - `context.rs` — `CompileContext` 编译上下文（变量/标签/节点ID管理）

pub mod context;
pub mod ir;

use std::sync::Arc;

use rust_agent_workflow::builder::WorkflowBuilder;
use rust_agent_workflow::executor::{
    AgentExecutor, ContextFunctionExecutor, FunctionExecutor, HandlerResult, HumanTaskExecutor,
    IExecutor,
};
use rust_agent_workflow::graph::{ComparisonOp, LoopConfig, VariableCondition};
use rust_agent_workflow::graph::edge::IEdgeCondition;
use rust_agent_workflow::WorkflowGraph;

use crate::actions::ActionDecl;
use crate::error::DeclError;
use crate::resolver::agent_resolver::AgentResolver;

use crate::error::Result;
use context::CompileContext;
use ir::{CompileNode, ExecutorKind};

/// 将动作列表编译为 WorkflowGraph。
pub fn compile_workflow(
    data: &crate::workflow_decl::WorkflowAgentData,
    agent_resolver: &mut AgentResolver,
) -> Result<WorkflowGraph> {
    let trigger_kind = data.trigger.kind.clone();
    let mut ctx = CompileContext::new(trigger_kind);

    let ir = compile_actions(&data.trigger.actions, &mut ctx)?;
    emit_ir(ir, &mut ctx, agent_resolver)
}

// ═══════════════════════════════════════════════════
// Pass 1: ActionDecl → CompileNode
// ═══════════════════════════════════════════════════

pub fn compile_actions(actions: &[ActionDecl], ctx: &mut CompileContext) -> Result<CompileNode> {
    let mut nodes: Vec<CompileNode> = Vec::new();

    for action in actions {
        let node = compile_one_action(action, ctx)?;
        nodes.push(node);
    }

    match nodes.len() {
        0 => Ok(CompileNode::NoOp),
        1 => Ok(nodes.into_iter().next().unwrap()),
        _ => Ok(CompileNode::Sequential(nodes)),
    }
}

fn compile_one_action(action: &ActionDecl, ctx: &mut CompileContext) -> Result<CompileNode> {
    match action {
        // ── 变量管理 ──
        ActionDecl::SetVariable {
            id,
            variable,
            value,
            ..
        } => {
            let node_id = id
                .clone()
                .unwrap_or_else(|| ctx.next_node_id("var_set"));
            ctx.variable_nodes
                .insert(variable.clone(), node_id.clone());
            Ok(CompileNode::Atomic {
                node_id,
                executor_kind: ExecutorKind::SetVariable {
                    variable: variable.clone(),
                    value: value.clone(),
                },
                is_output: false,
            })
        }

        ActionDecl::SetMultipleVariables {
            id, variables, ..
        } => {
            let node_id = id
                .clone()
                .unwrap_or_else(|| ctx.next_node_id("var_multi"));
            for v in variables.keys() {
                ctx.variable_nodes
                    .insert(v.clone(), node_id.clone());
            }
            Ok(CompileNode::Atomic {
                node_id,
                executor_kind: ExecutorKind::SetMultipleVariables {
                    variables: variables.clone(),
                },
                is_output: false,
            })
        }

        ActionDecl::SetTextVariable {
            id,
            variable,
            value,
            ..
        } => {
            let node_id = id
                .clone()
                .unwrap_or_else(|| ctx.next_node_id("var_text"));
            ctx.variable_nodes
                .insert(variable.clone(), node_id.clone());
            Ok(CompileNode::Atomic {
                node_id,
                executor_kind: ExecutorKind::SetVariable {
                    variable: variable.clone(),
                    value: serde_json::Value::String(value.clone()),
                },
                is_output: false,
            })
        }

        ActionDecl::ResetVariable { id, variable, .. } => {
            let node_id = id
                .clone()
                .unwrap_or_else(|| ctx.next_node_id("var_reset"));
            Ok(CompileNode::Atomic {
                node_id,
                executor_kind: ExecutorKind::ResetVariable {
                    variable: variable.clone(),
                },
                is_output: false,
            })
        }

        ActionDecl::ClearAllVariables { id, .. } => {
            let node_id = id
                .clone()
                .unwrap_or_else(|| ctx.next_node_id("var_clear"));
            Ok(CompileNode::Atomic {
                node_id,
                executor_kind: ExecutorKind::ClearAllVariables,
                is_output: false,
            })
        }

        ActionDecl::ParseValue {
            id,
            source,
            variable,
            ..
        } => {
            let node_id = id
                .clone()
                .unwrap_or_else(|| ctx.next_node_id("var_parse"));
            Ok(CompileNode::Atomic {
                node_id,
                executor_kind: ExecutorKind::ParseValue {
                    source: source.clone(),
                    target: variable.clone(),
                },
                is_output: false,
            })
        }

        ActionDecl::EditTableV2 {
            id,
            table,
            operation,
            row,
            ..
        } => {
            let node_id = id
                .clone()
                .unwrap_or_else(|| ctx.next_node_id("table_edit"));
            Ok(CompileNode::Atomic {
                node_id,
                executor_kind: ExecutorKind::EditTable {
                    table: table.clone(),
                    operation: operation.clone(),
                    row: row.clone(),
                },
                is_output: false,
            })
        }

        // ── AI 与输出 ──
        ActionDecl::InvokeAgent { id, agent, .. } => {
            let node_id = id
                .clone()
                .unwrap_or_else(|| {
                    ctx.next_node_id(&format!("agent_{}", agent.name))
                });
            ctx.label_targets
                .insert(agent.name.clone(), node_id.clone());
            Ok(CompileNode::Atomic {
                node_id,
                executor_kind: ExecutorKind::Agent(agent.name.clone()),
                is_output: true, // Agent 输出默认为工作流输出
            })
        }

        ActionDecl::SendActivity { id, activity, .. } => {
            let node_id = id
                .clone()
                .unwrap_or_else(|| ctx.next_node_id("send_activity"));
            Ok(CompileNode::Atomic {
                node_id,
                executor_kind: ExecutorKind::SendActivity {
                    text: activity.text.clone(),
                },
                is_output: true,
            })
        }

        ActionDecl::InvokeFunctionTool {
            id,
            function_name,
            arguments,
            output,
            ..
        } => {
            let node_id = id
                .clone()
                .unwrap_or_else(|| ctx.next_node_id("tool_call"));
            Ok(CompileNode::Atomic {
                node_id,
                executor_kind: ExecutorKind::ToolCall {
                    function_name: function_name.clone(),
                    arguments: arguments.clone().unwrap_or_default(),
                    output_variable: output.as_ref().and_then(|o| o.result.clone()),
                },
                is_output: true,
            })
        }

        // ── 控制流 ──
        ActionDecl::If {
            id,
            condition,
            then_actions,
            else_actions,
            ..
        } => {
            let cond_id = id
                .clone()
                .unwrap_or_else(|| ctx.next_node_id("if_cond"));
            let then_node = compile_actions(then_actions, ctx)?;
            let else_node = else_actions
                .as_ref()
                .map(|a| compile_actions(a, ctx))
                .transpose()?;
            Ok(CompileNode::Branch {
                condition_node_id: cond_id,
                condition: condition.clone(),
                true_branch: Box::new(then_node),
                false_branch: else_node.map(Box::new),
            })
        }

        ActionDecl::ConditionGroup {
            id,
            conditions,
            else_actions,
            ..
        } => {
            let cond_id = id
                .clone()
                .unwrap_or_else(|| ctx.next_node_id("cond_group"));
            let mut branches: Vec<(String, CompileNode)> = Vec::new();
            for branch in conditions {
                let sub = compile_actions(&branch.actions, ctx)?;
                branches.push((branch.condition.clone(), sub));
            }
            let else_node = else_actions
                .as_ref()
                .map(|a| compile_actions(a, ctx))
                .transpose()?;
            Ok(CompileNode::MultiBranch {
                condition_node_id: cond_id,
                branches,
                else_branch: else_node.map(Box::new),
            })
        }

        ActionDecl::Foreach {
            id,
            source,
            item_name,
            index_name,
            actions,
            ..
        } => {
            let entry_id = id
                .clone()
                .unwrap_or_else(|| ctx.next_node_id("foreach"));
            let body = compile_actions(actions, ctx)?;
            Ok(CompileNode::Loop {
                entry_node_id: entry_id,
                source: source.clone(),
                item_name: item_name.clone().unwrap_or_else(|| "item".to_string()),
                index_name: index_name.clone().unwrap_or_else(|| "index".to_string()),
                body: Box::new(body),
                max_iterations: 1000, // 安全上限
            })
        }

        ActionDecl::GotoAction { id, action_id } => {
            let node_id = id
                .clone()
                .unwrap_or_else(|| ctx.next_node_id("goto"));
            ctx.pending_gotos
                .push((node_id.clone(), action_id.clone()));
            Ok(CompileNode::Atomic {
                node_id,
                executor_kind: ExecutorKind::NoOp,
                is_output: false,
            })
        }

        ActionDecl::BreakLoop => Ok(CompileNode::Terminate),
        ActionDecl::ContinueLoop => Ok(CompileNode::Continue),

        // ── 人机交互 ──
        ActionDecl::Question {
            id,
            question,
            variable,
            ..
        } => {
            let node_id = id
                .clone()
                .unwrap_or_else(|| ctx.next_node_id("question"));
            let form = serde_json::json!({
                "type": "question",
                "text": question.text,
                "variable": variable
            });
            Ok(CompileNode::Atomic {
                node_id,
                executor_kind: ExecutorKind::HumanTask(form),
                is_output: true,
            })
        }

        ActionDecl::RequestExternalInput {
            id,
            prompt,
            variable,
            ..
        } => {
            let node_id = id
                .clone()
                .unwrap_or_else(|| ctx.next_node_id("external_input"));
            let form = serde_json::json!({
                "type": "external_input",
                "text": prompt.text,
                "variable": variable
            });
            Ok(CompileNode::Atomic {
                node_id,
                executor_kind: ExecutorKind::HumanTask(form),
                is_output: true,
            })
        }

        // ── HTTP / MCP ──
        ActionDecl::HttpRequestAction {
            id,
            url,
            method,
            headers,
            body,
            response,
            ..
        } => {
            let node_id = id
                .clone()
                .unwrap_or_else(|| ctx.next_node_id("http"));
            Ok(CompileNode::Atomic {
                node_id,
                executor_kind: ExecutorKind::HttpRequest {
                    url: url.clone(),
                    method: method.clone(),
                    headers: headers.clone().unwrap_or_default(),
                    body: body
                        .as_ref()
                        .map(|b| match b {
                            crate::actions::HttpBody::Json { value } => value.to_string(),
                            crate::actions::HttpBody::Raw { value } => value.clone(),
                            crate::actions::HttpBody::None => String::new(),
                        })
                        .unwrap_or_default(),
                    response_variable: response.clone(),
                },
                is_output: true,
            })
        }

        ActionDecl::InvokeMcpTool {
            id,
            server_url,
            tool_name,
            arguments,
            output,
            ..
        } => {
            let node_id = id
                .clone()
                .unwrap_or_else(|| ctx.next_node_id("mcp"));
            Ok(CompileNode::Atomic {
                node_id,
                executor_kind: ExecutorKind::McpRequest {
                    server_url: server_url.clone(),
                    tool_name: tool_name.clone(),
                    arguments: arguments.clone().unwrap_or_default(),
                    output_variable: output.as_ref().and_then(|o| o.result.clone()),
                },
                is_output: true,
            })
        }

        // ── 终端与对话 ──
        ActionDecl::EndWorkflow { id, .. } => {
            let node_id = id
                .clone()
                .unwrap_or_else(|| ctx.next_node_id("end_wf"));
            Ok(CompileNode::Atomic {
                node_id,
                executor_kind: ExecutorKind::EndWorkflow,
                is_output: true,
            })
        }

        ActionDecl::EndConversation { id, .. } => {
            let node_id = id
                .clone()
                .unwrap_or_else(|| ctx.next_node_id("end_conv"));
            Ok(CompileNode::Atomic {
                node_id,
                executor_kind: ExecutorKind::EndWorkflow,
                is_output: true,
            })
        }

        ActionDecl::CreateConversation {
            id,
            conversation_id,
            ..
        } => {
            let node_id = id
                .clone()
                .unwrap_or_else(|| ctx.next_node_id("create_conv"));
            Ok(CompileNode::Atomic {
                node_id,
                executor_kind: ExecutorKind::CreateConversation {
                    conversation_id: conversation_id.clone(),
                },
                is_output: false,
            })
        }

        ActionDecl::AddConversationMessage { id, message } => {
            let node_id = id
                .clone()
                .unwrap_or_else(|| ctx.next_node_id("add_msg"));
            Ok(CompileNode::Atomic {
                node_id,
                executor_kind: ExecutorKind::AddMessage {
                    role: message.role.clone().unwrap_or_else(|| "user".to_string()),
                    content: message.content.clone(),
                },
                is_output: false,
            })
        }

        // BreakLoop / ContinueLoop 顶层出现（不在 Foreach 内部）
        _ => Ok(CompileNode::NoOp),
    }
}

// ═══════════════════════════════════════════════════
// Pass 2: CompileNode → WorkflowGraph
// ═══════════════════════════════════════════════════

pub fn emit_ir(
    root: CompileNode,
    ctx: &mut CompileContext,
    agent_resolver: &mut AgentResolver,
) -> Result<WorkflowGraph> {
    let mut builder = WorkflowBuilder::new();
    let (first_id, last_id) = emit_node(&root, &mut builder, ctx, agent_resolver, None)?;

    if let Some(ref first) = first_id {
        builder = builder.set_start(first.clone());
    }

    // 回填 GotoAction 的延迟边
    for (from_id, target_label) in &ctx.pending_gotos {
        if let Some(target_id) = ctx.label_targets.get(target_label) {
            builder = builder.add_edge(from_id.clone(), target_id.clone());
        } else {
            return Err(DeclError::Resolution(format!(
                "GotoAction target '{}' not found (from '{}')",
                target_label, from_id
            )));
        }
    }

    builder.build().map_err(|e| {
        DeclError::Resolution(format!("Failed to build workflow graph: {}", e))
    })
}

/// 递归排放 CompileNode 树到 WorkflowBuilder。
///
/// 返回 `(first_node_id, last_node_id)` 用于父节点连接。
fn emit_node(
    node: &CompileNode,
    builder: &mut WorkflowBuilder,
    ctx: &mut CompileContext,
    agent_resolver: &mut AgentResolver,
    loopback_target: Option<String>,
) -> Result<(Option<String>, Option<String>)> {
    match node {
        CompileNode::NoOp => Ok((None, None)),

        CompileNode::Terminate => Ok((None, None)),

        CompileNode::Continue => {
            if let Some(target) = loopback_target {
                return Ok((None, Some(target)));
            }
            Ok((None, None))
        }

        CompileNode::Atomic {
            node_id,
            executor_kind,
            is_output,
        } => {
            let executor = build_executor(node_id, executor_kind, ctx);
            *builder = builder.add_node(node_id.clone(), Arc::new(executor));
            if *is_output {
                *builder = builder.with_output_from(node_id.clone());
            }
            Ok((Some(node_id.clone()), Some(node_id.clone())))
        }

        CompileNode::Sequential(children) => {
            let mut first: Option<String> = None;
            let mut prev: Option<String> = None;

            for child in children {
                let (child_first, child_last) =
                    emit_node(child, builder, ctx, agent_resolver, loopback_target.clone())?;
                if let Some(cf) = child_first {
                    if first.is_none() {
                        first = Some(cf);
                    }
                }
                if let Some(cl) = child_last {
                    if let Some(ref p) = prev {
                        *builder = builder.add_edge(p.clone(), cl.clone());
                    }
                    prev = Some(cl);
                }
            }
            Ok((first, prev))
        }

        CompileNode::Branch {
            condition_node_id,
            condition,
            true_branch,
            false_branch,
        } => {
            // 条件求值节点
            let cond_exec = build_condition_executor(condition_node_id, condition, ctx);
            *builder = builder.add_node(condition_node_id.clone(), Arc::new(cond_exec));

            // 编译 true 分支
            let (true_first, true_last) =
                emit_node(true_branch, builder, ctx, loopback_target.clone())?;

            // 编译 false 分支
            let (false_first, false_last) = false_branch
                .as_ref()
                .map(|f| emit_node(f, builder, ctx, loopback_target.clone()))
                .transpose()?;

            // 使用 exclusive_gateway
            if let Some(tf) = true_first {
                let true_cond = Arc::new(VariableCondition::new(
                    condition_node_id.clone(),
                    ComparisonOp::Eq,
                    serde_json::json!(true),
                ));
                if let Some((ff, _)) = &false_first {
                    *builder = builder.exclusive_gateway(
                        condition_node_id.clone(),
                        vec![(tf, true_cond)],
                        Some(ff.clone()),
                    );
                } else {
                    *builder = builder.exclusive_gateway(
                        condition_node_id.clone(),
                        vec![(tf, true_cond)],
                        None::<String>,
                    );
                }
            }

            // 合并 last
            let last = true_last
                .or(false_last.as_ref().and_then(|(_, l)| l.clone()));
            Ok((Some(condition_node_id.clone()), last))
        }

        CompileNode::MultiBranch {
            condition_node_id,
            branches,
            else_branch,
        } => {
            let cond_exec = build_multi_condition_executor(
                condition_node_id,
                branches,
                ctx,
            );
            *builder = builder.add_node(condition_node_id.clone(), Arc::new(cond_exec));

            // 编译每个分支
            let mut branch_starts: Vec<(String, Arc<dyn rust_agent_workflow::graph::edge::IEdgeCondition>)> = Vec::new();
            let mut fallback: Option<String> = None;

            for (i, (_, sub_node)) in branches.iter().enumerate() {
                let (bf, bl) =
                    emit_node(sub_node, builder, ctx, loopback_target.clone())?;
                if let Some(bf_id) = bf {
                    let cond = Arc::new(VariableCondition::new(
                        condition_node_id.clone(),
                        ComparisonOp::Eq,
                        serde_json::json!(i),
                    ));
                    branch_starts.push((bf_id, cond));
                }
                if fallback.is_none() {
                    fallback = bl;
                }
            }

            if let Some(eb) = else_branch {
                let (ef, _) = emit_node(eb, builder, ctx, loopback_target.clone())?;
                if let Some(ef_id) = ef {
                    fallback = Some(ef_id);
                }
            }

            if !branch_starts.is_empty() {
                *builder = builder.exclusive_gateway(
                    condition_node_id.clone(),
                    branch_starts,
                    fallback,
                );
            }

            Ok((Some(condition_node_id.clone()), fallback))
        }

        CompileNode::Loop {
            entry_node_id,
            source,
            item_name: _,
            index_name: _,
            body,
            max_iterations,
        } => {
            let loop_config = LoopConfig::new(*max_iterations)
                .with_variable(format!("__loop_{}", entry_node_id));

            // 入口节点：从 source 变量读取集合
            let entry_exec = build_loop_entry_executor(entry_node_id, source, ctx);
            *builder = builder.add_node(entry_node_id.clone(), Arc::new(entry_exec));
            // 注意：with_loop 需要在 add_node 之后通过 builder 的 with_loop 设置
            // 由于 builder 是 builder-pattern，我们需要用其他方式标记 loop_config
            // 这里简化为：loop_entry node 本身是 FunctionExecutor，在 handle 中管理迭代

            let (body_first, body_last) =
                emit_node(body, builder, ctx, Some(entry_node_id.clone()))?;

            if let Some(bf) = body_first {
                *builder = builder.add_edge(entry_node_id.clone(), bf);
            }
            if let Some(bl) = body_last {
                // 循环回边
                *builder = builder.add_loopback_edge(bl, entry_node_id.clone());
            }

            Ok((Some(entry_node_id.clone()), Some(entry_node_id.clone())))
        }
    }
}

// ═══════════════════════════════════════════════════
// Executor 构建辅助
// ═══════════════════════════════════════════════════

fn build_executor(
    node_id: &str,
    kind: &ExecutorKind,
    ctx: &mut CompileContext,
) -> Box<dyn crate::executor::IExecutor> {
    match kind {
        ExecutorKind::Agent(name) => {
            if let Some(resolver) = &ctx.agent_resolver {
                if let Some(agent) = resolver.get_agent(name) {
                    return Box::new(AgentExecutor::new(node_id, agent));
                }
            }
            // Fallback: empty executor
            Box::new(crate::executor::FunctionExecutor::new(
                node_id,
                |_: String| -> Vec<String> { vec![format!("[Agent {} not found]", name)] },
            ))
        }

        ExecutorKind::SetVariable { variable, value } => {
            let var = variable.clone();
            let val = value.clone();
            Box::new(ContextFunctionExecutor::new(node_id, move |_msg, ctx, _prog| {
                let var = var.clone();
                let val = val.clone();
                async move {
                    ctx.write_state(&var, val).await?;
                    Ok(HandlerResult::None)
                }
            }))
        }

        ExecutorKind::SetMultipleVariables { variables } => {
            let vars = variables.clone();
            Box::new(ContextFunctionExecutor::new(node_id, move |_msg, ctx, _prog| {
                let vars = vars.clone();
                async move {
                    for (k, v) in &vars {
                        ctx.write_state(k, v.clone()).await?;
                    }
                    Ok(HandlerResult::None)
                }
            }))
        }

        ExecutorKind::ResetVariable { variable } => {
            let var = variable.clone();
            Box::new(ContextFunctionExecutor::new(node_id, move |_msg, ctx, _prog| {
                let var = var.clone();
                async move {
                    ctx.clear_state(&var).await?;
                    Ok(HandlerResult::None)
                }
            }))
        }

        ExecutorKind::ClearAllVariables => {
            Box::new(ContextFunctionExecutor::new(node_id, move |_msg, ctx, _prog| {
                async move {
                    let names = ctx.variable_names().await;
                    for name in names {
                        ctx.clear_state(&name).await?;
                    }
                    Ok(HandlerResult::None)
                }
            }))
        }

        ExecutorKind::ParseValue { source, target } => {
            let src = source.clone();
            let tgt = target.clone();
            Box::new(ContextFunctionExecutor::new(node_id, move |_msg, ctx, _prog| {
                let src = src.clone();
                let tgt = tgt.clone();
                async move {
                    if let Some(val) = ctx.read_state(&src).await? {
                        ctx.write_state(&tgt, val).await?;
                    }
                    Ok(HandlerResult::None)
                }
            }))
        }

        ExecutorKind::EditTable { table, operation, row } => {
            let tbl = table.clone();
            let op = operation.clone();
            let r = row.clone();
            Box::new(ContextFunctionExecutor::new(node_id, move |_msg, ctx, _prog| {
                let tbl = tbl.clone();
                let op = op.clone();
                let r = r.clone();
                async move {
                    let mut current = ctx
                        .read_state(&tbl)
                        .await?
                        .unwrap_or_else(|| serde_json::Value::Array(vec![]));
                    match op.as_str() {
                        "add" => {
                            if let Some(arr) = current.as_array_mut() {
                                arr.push(serde_json::to_value(&r).unwrap_or_default());
                            }
                        }
                        "update" => {
                            // Simple update: replace matching rows
                            if let Some(arr) = current.as_array_mut() {
                                for item in arr.iter_mut() {
                                    if item == &serde_json::to_value(&r).unwrap_or_default() {
                                        *item = serde_json::to_value(&r).unwrap_or_default();
                                    }
                                }
                            }
                        }
                        "delete" => {
                            if let Some(arr) = current.as_array_mut() {
                                arr.retain(|item| {
                                    item != &serde_json::to_value(&r).unwrap_or_default()
                                });
                            }
                        }
                        _ => {}
                    }
                    ctx.write_state(&tbl, current).await?;
                    Ok(HandlerResult::None)
                }
            }))
        }

        ExecutorKind::SendActivity { text } => {
            let txt = text.clone();
            Box::new(ContextFunctionExecutor::new(node_id, move |_msg, ctx, _prog| {
                let txt = txt.clone();
                async move {
                    ctx.write_state("__last_activity", txt).await?;
                    Ok(HandlerResult::Messages(vec![Arc::new("activity_sent".to_string())]))
                }
            }))
        }

        ExecutorKind::ToolCall {
            function_name,
            arguments,
            output_variable,
        } => {
            let fname = function_name.clone();
            let args = arguments.clone();
            let out_var = output_variable.clone();
            Box::new(FunctionExecutor::new(node_id, move |_: String| -> Vec<String> {
                vec![format!("[Tool {} called with {:?}]", fname, args)]
            }))
        }

        ExecutorKind::HumanTask(form) => {
            let f = form.clone();
            Box::new(HumanTaskExecutor::new(
                node_id,
                Arc::new(move |_ctx| f.clone()),
            ))
        }

        ExecutorKind::HttpRequest {
            url,
            method,
            headers: _,
            body: _,
            response_variable: _,
        } => {
            let u = url.clone();
            let m = method.clone();
            Box::new(FunctionExecutor::new(node_id, move |_: String| -> Vec<String> {
                vec![format!("[HTTP {} {} called]", m, u)]
            }))
        }

        ExecutorKind::McpRequest {
            server_url,
            tool_name,
            arguments: _,
            output_variable: _,
        } => {
            let srv = server_url.clone();
            let tname = tool_name.clone();
            Box::new(FunctionExecutor::new(node_id, move |_: String| -> Vec<String> {
                vec![format!("[MCP {} at {} called]", tname, srv)]
            }))
        }

        ExecutorKind::EndWorkflow => Box::new(FunctionExecutor::new(
            node_id,
            |_: String| -> Vec<String> { vec!["workflow_ended".to_string()] },
        )),

        ExecutorKind::CreateConversation { conversation_id } => {
            let cid = conversation_id.clone();
            Box::new(FunctionExecutor::new(node_id, move |_: String| -> Vec<String> {
                vec![format!("[Conversation {} created]", cid)]
            }))
        }

        ExecutorKind::AddMessage { role, content } => {
            let r = role.clone();
            let c = content.clone();
            Box::new(FunctionExecutor::new(node_id, move |_: String| -> Vec<String> {
                vec![format!("[{}]: {}", r, c)]
            }))
        }

        ExecutorKind::NoOp => Box::new(FunctionExecutor::new(
            node_id,
            |_: String| -> Vec<String> { vec![] },
        )),
    }
}

fn build_condition_executor(
    node_id: &str,
    condition: &str,
    _ctx: &mut CompileContext,
) -> Box<dyn crate::executor::IExecutor> {
    let cond = condition.clone();
    Box::new(FunctionExecutor::new(node_id, move |_msg: String| -> Vec<String> {
        // Simple expression evaluation
        let result = evaluate_simple_condition(&cond);
        vec![if result {
            "true".to_string()
        } else {
            "false".to_string()
        }]
    }))
}

fn build_multi_condition_executor(
    node_id: &str,
    branches: &[(String, CompileNode)],
    _ctx: &mut CompileContext,
) -> Box<dyn crate::executor::IExecutor> {
    let conditions: Vec<String> = branches.iter().map(|(c, _)| c.clone()).collect();
    Box::new(
        FunctionExecutor::new(node_id, move |_msg: String| -> Vec<String> {
            for (i, cond) in conditions.iter().enumerate() {
                if evaluate_simple_condition(cond) {
                    return vec![i.to_string()];
                }
            }
            vec!["-1".to_string()]
        }),
    )
}

fn build_loop_entry_executor(
    node_id: &str,
    source_var: &str,
    _ctx: &mut CompileContext,
) -> Box<dyn crate::executor::IExecutor> {
    let sv = source_var.clone();
    Box::new(ContextFunctionExecutor::new(node_id, move |_msg, ctx, _prog| {
        let sv = sv.clone();
        async move {
            let collection = ctx
                .read_state(&sv)
                .await?
                .unwrap_or_else(|| serde_json::Value::Array(vec![]));
            Ok(HandlerResult::Messages(vec![Arc::new(collection)]))
        }
    }))
}

/// 简单表达式求值，支持 ==, !=, >=, <=, >, <, contains。
///
/// 后续可集成 PowerFx 或 Rhai 完整表达式引擎。
fn evaluate_simple_condition(expr: &str) -> bool {
    let expr = expr.trim();
    // Strip leading '=' if present (PowerFx style)
    let expr = expr.strip_prefix('=').unwrap_or(expr);

    // Try contains
    if expr.contains(" contains ") {
        let parts: Vec<&str> = expr.splitn(2, " contains ").collect();
        if parts.len() == 2 {
            return parts[0].contains(parts[1]);
        }
    }

    // Try >=
    if expr.contains(" >= ") {
        let parts: Vec<&str> = expr.splitn(2, " >= ").collect();
        if let (Ok(a), Ok(b)) = (parts[0].trim().parse::<f64>(), parts[1].trim().parse::<f64>()) {
            return a >= b;
        }
    }

    // Try <=
    if expr.contains(" <= ") {
        let parts: Vec<&str> = expr.splitn(2, " <= ").collect();
        if let (Ok(a), Ok(b)) = (parts[0].trim().parse::<f64>(), parts[1].trim().parse::<f64>()) {
            return a <= b;
        }
    }

    // Try >
    if expr.contains(" > ") {
        let parts: Vec<&str> = expr.splitn(2, " > ").collect();
        if let (Ok(a), Ok(b)) = (parts[0].trim().parse::<f64>(), parts[1].trim().parse::<f64>()) {
            return a > b;
        }
    }

    // Try <
    if expr.contains(" < ") {
        let parts: Vec<&str> = expr.splitn(2, " < ").collect();
        if let (Ok(a), Ok(b)) = (parts[0].trim().parse::<f64>(), parts[1].trim().parse::<f64>()) {
            return a < b;
        }
    }

    // Try !=
    if expr.contains(" != ") {
        let parts: Vec<&str> = expr.splitn(2, " != ").collect();
        return parts[0].trim() != parts[1].trim();
    }

    // Try ==
    if expr.contains(" == ") {
        let parts: Vec<&str> = expr.splitn(2, " == ").collect();
        return parts[0].trim() == parts[1].trim();
    }

    // Try boolean literal
    match expr.to_lowercase().as_str() {
        "true" | "yes" | "1" => return true,
        "false" | "no" | "0" => return false,
        _ => {}
    }

    // Non-empty string is truthy
    !expr.is_empty()
}
