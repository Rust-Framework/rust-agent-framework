//! Declaration Workflow Compiler
//!
//! ??MAF-aligned `ActionDecl` ??????????`WorkflowGraph`??
//!
//! ## ??
//!
//! ```ignore
//! ActionDecl[] ??CompileNode (IR) ??WorkflowGraph
//! ```
//!
//! - `ir.rs` ??`CompileNode` ???? + `ExecutorKind` ??????
//! - `context.rs` ??`CompileContext` ????????/??/??ID????

// TODO: 迁移编译器以使用 DeclAgentBuilder 替代已废弃的 AgentResolver
#![allow(deprecated)]

pub mod context;
pub mod ir;

use std::sync::Arc;

use rust_agent_workflow::builder::WorkflowBuilder;
use rust_agent_workflow::executor::{
    AgentExecutor, ContextFunctionExecutor, FunctionExecutor, HandlerResult, HumanTaskExecutor,
    IExecutor,
};
use rust_agent_workflow::graph::{ComparisonOp, LoopConfig, VariableEdgeCondition};
use rust_agent_workflow::WorkflowGraph;

use crate::actions::ActionDecl;
use crate::error::{DeclError, Result};
use crate::resolver::agent_resolver::AgentResolver;

use context::CompileContext;
use ir::{CompileNode, ExecutorKind};

/// ???????? WorkflowGraph??
pub fn compile_workflow(
    data: &crate::workflow_decl::WorkflowAgentData,
    agent_resolver: &mut AgentResolver,
) -> Result<WorkflowGraph> {
    let mut ctx = CompileContext::new(data.trigger.kind.clone());

    let ir = compile_actions(&data.trigger.actions, &mut ctx)?;
    emit_ir(ir, &mut ctx, agent_resolver)
}

// ????????????????????????????????????????????????????
// Pass 1: ActionDecl ??CompileNode
// ????????????????????????????????????????????????????

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
        // ?? ???? ??
        ActionDecl::SetVariable { id, variable, value, .. } => {
            let node_id = id.clone().unwrap_or_else(|| ctx.next_node_id("var_set"));
            ctx.variable_nodes.insert(variable.clone(), node_id.clone());
            Ok(CompileNode::Atomic {
                node_id,
                executor_kind: ExecutorKind::SetVariable { variable: variable.clone(), value: value.clone() },
                is_output: false,
            })
        }

        ActionDecl::SetMultipleVariables { id, variables, .. } => {
            let node_id = id.clone().unwrap_or_else(|| ctx.next_node_id("var_multi"));
            for v in variables.keys() { ctx.variable_nodes.insert(v.clone(), node_id.clone()); }
            Ok(CompileNode::Atomic {
                node_id,
                executor_kind: ExecutorKind::SetMultipleVariables { variables: variables.clone() },
                is_output: false,
            })
        }

        ActionDecl::SetTextVariable { id, variable, value, .. } => {
            let node_id = id.clone().unwrap_or_else(|| ctx.next_node_id("var_text"));
            ctx.variable_nodes.insert(variable.clone(), node_id.clone());
            Ok(CompileNode::Atomic {
                node_id,
                executor_kind: ExecutorKind::SetVariable { variable: variable.clone(), value: serde_json::Value::String(value.clone()) },
                is_output: false,
            })
        }

        ActionDecl::ResetVariable { id, variable, .. } => {
            let node_id = id.clone().unwrap_or_else(|| ctx.next_node_id("var_reset"));
            Ok(CompileNode::Atomic { node_id, executor_kind: ExecutorKind::ResetVariable { variable: variable.clone() }, is_output: false })
        }

        ActionDecl::ClearAllVariables { id, .. } => {
            let node_id = id.clone().unwrap_or_else(|| ctx.next_node_id("var_clear"));
            Ok(CompileNode::Atomic { node_id, executor_kind: ExecutorKind::ClearAllVariables, is_output: false })
        }

        ActionDecl::ParseValue { id, source, variable, .. } => {
            let node_id = id.clone().unwrap_or_else(|| ctx.next_node_id("var_parse"));
            Ok(CompileNode::Atomic { node_id, executor_kind: ExecutorKind::ParseValue { source: source.clone(), target: variable.clone() }, is_output: false })
        }

        ActionDecl::EditTableV2 { id, table, operation, row, .. } => {
            let node_id = id.clone().unwrap_or_else(|| ctx.next_node_id("table_edit"));
            Ok(CompileNode::Atomic { node_id, executor_kind: ExecutorKind::EditTable { table: table.clone(), operation: operation.clone(), row: row.clone() }, is_output: false })
        }

        // ?? AI ??????
        ActionDecl::InvokeAgent { id, agent, .. } => {
            let node_id = id.clone().unwrap_or_else(|| ctx.next_node_id(&format!("agent_{}", agent.name)));
            ctx.label_targets.insert(agent.name.clone(), node_id.clone());
            Ok(CompileNode::Atomic { node_id, executor_kind: ExecutorKind::Agent(agent.name.clone()), is_output: true })
        }

        ActionDecl::SendActivity { id, activity, .. } => {
            let node_id = id.clone().unwrap_or_else(|| ctx.next_node_id("send_activity"));
            Ok(CompileNode::Atomic { node_id, executor_kind: ExecutorKind::SendActivity { text: activity.text.clone() }, is_output: true })
        }

        ActionDecl::InvokeFunctionTool { id, function_name, arguments, output, .. } => {
            let node_id = id.clone().unwrap_or_else(|| ctx.next_node_id("tool_call"));
            Ok(CompileNode::Atomic {
                node_id,
                executor_kind: ExecutorKind::ToolCall { function_name: function_name.clone(), arguments: arguments.clone().unwrap_or_default(), output_variable: output.as_ref().and_then(|o| o.result.clone()) },
                is_output: true,
            })
        }

        // ?? ??????
        ActionDecl::If { id, condition, then_actions, else_actions, .. } => {
            let cond_id = id.clone().unwrap_or_else(|| ctx.next_node_id("if_cond"));
            let then_node = compile_actions(then_actions, ctx)?;
            let else_node = else_actions.as_ref().map(|a| compile_actions(a, ctx)).transpose()?;
            Ok(CompileNode::Branch { condition_node_id: cond_id, condition: condition.clone(), true_branch: Box::new(then_node), false_branch: else_node.map(Box::new) })
        }

        ActionDecl::ConditionGroup { id, conditions, else_actions, .. } => {
            let cond_id = id.clone().unwrap_or_else(|| ctx.next_node_id("cond_group"));
            let mut branches: Vec<(String, CompileNode)> = Vec::new();
            for branch in conditions {
                branches.push((branch.condition.clone(), compile_actions(&branch.actions, ctx)?));
            }
            let else_node = else_actions.as_ref().map(|a| compile_actions(a, ctx)).transpose()?;
            Ok(CompileNode::MultiBranch { condition_node_id: cond_id, branches, else_branch: else_node.map(Box::new) })
        }

        ActionDecl::Foreach { id, source, item_name, index_name, actions, .. } => {
            let entry_id = id.clone().unwrap_or_else(|| ctx.next_node_id("foreach"));
            let body = compile_actions(actions, ctx)?;
            Ok(CompileNode::Loop {
                entry_node_id: entry_id,
                source: source.clone(),
                item_name: item_name.clone().unwrap_or_else(|| "item".to_string()),
                index_name: index_name.clone().unwrap_or_else(|| "index".to_string()),
                body: Box::new(body),
                max_iterations: 1000,
            })
        }

        ActionDecl::GotoAction { id, action_id } => {
            let node_id = id.clone().unwrap_or_else(|| ctx.next_node_id("goto"));
            ctx.pending_gotos.push((node_id.clone(), action_id.clone()));
            Ok(CompileNode::Atomic { node_id, executor_kind: ExecutorKind::NoOp, is_output: false })
        }

        ActionDecl::BreakLoop => Ok(CompileNode::Terminate),
        ActionDecl::ContinueLoop => Ok(CompileNode::Continue),

        // ?? ???? ??
        ActionDecl::Question { id, question, variable, .. } => {
            let node_id = id.clone().unwrap_or_else(|| ctx.next_node_id("question"));
            let form = serde_json::json!({"type":"question","text":question.text,"variable":variable});
            Ok(CompileNode::Atomic { node_id, executor_kind: ExecutorKind::HumanTask(form), is_output: true })
        }

        ActionDecl::RequestExternalInput { id, prompt, variable, .. } => {
            let node_id = id.clone().unwrap_or_else(|| ctx.next_node_id("external_input"));
            let form = serde_json::json!({"type":"external_input","text":prompt.text,"variable":variable});
            Ok(CompileNode::Atomic { node_id, executor_kind: ExecutorKind::HumanTask(form), is_output: true })
        }

        // ?? HTTP / MCP ??
        ActionDecl::HttpRequestAction { id, url, method, headers, body, response, .. } => {
            let node_id = id.clone().unwrap_or_else(|| ctx.next_node_id("http"));
            let body_str = body.as_ref().map(|b| match b { crate::actions::HttpBody::Json { value } => value.to_string(), crate::actions::HttpBody::Raw { value } => value.clone(), crate::actions::HttpBody::None => String::new() }).unwrap_or_default();
            Ok(CompileNode::Atomic {
                node_id,
                executor_kind: ExecutorKind::HttpRequest { url: url.clone(), method: method.clone(), headers: headers.clone().unwrap_or_default(), body: body_str, response_variable: response.clone() },
                is_output: true,
            })
        }

        ActionDecl::InvokeMcpTool { id, server_url, tool_name, arguments, output, .. } => {
            let node_id = id.clone().unwrap_or_else(|| ctx.next_node_id("mcp"));
            Ok(CompileNode::Atomic {
                node_id,
                executor_kind: ExecutorKind::McpRequest { server_url: server_url.clone(), tool_name: tool_name.clone(), arguments: arguments.clone().unwrap_or_default(), output_variable: output.as_ref().and_then(|o| o.result.clone()) },
                is_output: true,
            })
        }

        // ?? ????????
        ActionDecl::EndWorkflow { id, .. } => {
            let node_id = id.clone().unwrap_or_else(|| ctx.next_node_id("end_wf"));
            Ok(CompileNode::Atomic { node_id, executor_kind: ExecutorKind::EndWorkflow, is_output: true })
        }

        ActionDecl::EndConversation { id, .. } => {
            let node_id = id.clone().unwrap_or_else(|| ctx.next_node_id("end_conv"));
            Ok(CompileNode::Atomic { node_id, executor_kind: ExecutorKind::EndWorkflow, is_output: true })
        }

        ActionDecl::CreateConversation { id, conversation_id, .. } => {
            let node_id = id.clone().unwrap_or_else(|| ctx.next_node_id("create_conv"));
            Ok(CompileNode::Atomic { node_id, executor_kind: ExecutorKind::CreateConversation { conversation_id: conversation_id.clone() }, is_output: false })
        }

        ActionDecl::AddConversationMessage { id, message } => {
            let node_id = id.clone().unwrap_or_else(|| ctx.next_node_id("add_msg"));
            Ok(CompileNode::Atomic {
                node_id,
                executor_kind: ExecutorKind::AddMessage { role: message.role.clone().unwrap_or_else(|| "user".to_string()), content: message.content.clone() },
                is_output: false,
            })
        }
    }
}

// ????????????????????????????????????????????????????
// Pass 2: CompileNode ??WorkflowGraph
// ????????????????????????????????????????????????????

pub fn emit_ir(
    root: CompileNode,
    ctx: &mut CompileContext,
    agent_resolver: &mut AgentResolver,
) -> Result<WorkflowGraph> {
    let mut builder = WorkflowBuilder::new();
    let (first_id, _last_id) = emit_node(&root, &mut builder, ctx, agent_resolver, None)?;

    if let Some(ref first) = first_id {
        builder = builder.set_start(first.clone());
    }

    for (from_id, target_label) in &ctx.pending_gotos {
        if let Some(target_id) = ctx.label_targets.get(target_label) {
            builder = builder.add_edge(from_id.clone(), target_id.clone());
        } else {
            return Err(DeclError::Resolution(format!(
                "GotoAction target '{}' not found (from '{}')", target_label, from_id
            )));
        }
    }

    builder.build().map_err(|e| DeclError::Resolution(format!("Failed to build workflow graph: {}", e)))
}

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

        CompileNode::Atomic { node_id, executor_kind, is_output } => {
            let executor = build_executor(node_id, executor_kind, agent_resolver);
            *builder = builder.clone().add_node(node_id.clone(), executor);
            if *is_output {
                *builder = builder.clone().with_output_from(node_id.clone());
            }
            Ok((Some(node_id.clone()), Some(node_id.clone())))
        }

        CompileNode::Sequential(children) => {
            let mut first: Option<String> = None;
            let mut prev: Option<String> = None;
            for child in children {
                let (cf, cl) = emit_node(child, builder, ctx, agent_resolver, loopback_target.clone())?;
                if first.is_none() { first = cf.clone(); }
                if let (Some(p), Some(c)) = (&prev, &cl) {
                    *builder = builder.clone().add_edge(p.clone(), c.clone());
                }
                prev = cl.or(cf);
            }
            Ok((first, prev))
        }

        CompileNode::Branch { condition_node_id, condition, true_branch, false_branch } => {
            let cond_exec = build_condition_executor(condition_node_id, condition);
            *builder = builder.clone().add_node(condition_node_id.clone(), cond_exec);

            let (true_first, true_last) = emit_node(true_branch, builder, ctx, agent_resolver, loopback_target.clone())?;
            let false_result = false_branch.as_ref().map(|f| emit_node(f, builder, ctx, agent_resolver, loopback_target.clone())).transpose()?;

            if let Some(tf) = true_first {
                let true_cond = Arc::new(VariableEdgeCondition::new(
                    condition_node_id.clone(), ComparisonOp::Eq, serde_json::json!(true)));
                match &false_result {
                    Some(fr) if fr.0.is_some() => {
                        *builder = builder.clone().exclusive_gateway(
                            condition_node_id.clone(), vec![(tf, true_cond)], Some(fr.0.as_ref().unwrap().clone()));
                    }
                    _ => {
                        *builder = builder.clone().exclusive_gateway(
                            condition_node_id.clone(), vec![(tf, true_cond)], None::<String>);
                    }
                }
            }

            let last = true_last.or(false_result.as_ref().and_then(|(l, _)| l.clone()));
            Ok((Some(condition_node_id.clone()), last))
        }

        CompileNode::MultiBranch { condition_node_id, branches, else_branch } => {
            let cond_exec = build_multi_condition_executor(condition_node_id, branches);
            *builder = builder.clone().add_node(condition_node_id.clone(), cond_exec);

            let mut branch_starts: Vec<(String, Arc<dyn rust_agent_workflow::graph::edge::IEdgeCondition>)> = Vec::new();
            let mut fallback: Option<String> = None;

            for (i, (_, sub_node)) in branches.iter().enumerate() {
                let (bf, bl) = emit_node(sub_node, builder, ctx, agent_resolver, loopback_target.clone())?;
                if let Some(bf_id) = bf {
                    let cond = Arc::new(VariableEdgeCondition::new(
                        condition_node_id.clone(), ComparisonOp::Eq, serde_json::json!(i)));
                    branch_starts.push((bf_id, cond));
                }
                if fallback.is_none() { fallback = bl; }
            }

            if let Some(eb) = else_branch {
                let (ef, _) = emit_node(eb, builder, ctx, agent_resolver, loopback_target.clone())?;
                if let Some(ef_id) = ef { fallback = Some(ef_id); }
            }

            if !branch_starts.is_empty() {
                *builder = builder.clone().exclusive_gateway(condition_node_id.clone(), branch_starts, fallback.clone());
            }
            Ok((Some(condition_node_id.clone()), fallback))
        }

        CompileNode::Loop { entry_node_id, source: _, item_name: _, index_name: _, body, max_iterations } => {
            let loop_config = LoopConfig::new(*max_iterations).with_variable(format!("__loop_{}", entry_node_id));
            let loop_exec = build_loop_entry_executor(entry_node_id);

            *builder = builder.clone().add_node(entry_node_id.clone(), loop_exec).with_loop(loop_config);

            let (body_first, body_last) = emit_node(body, builder, ctx, agent_resolver, Some(entry_node_id.clone()))?;
            if let Some(bf) = body_first { *builder = builder.clone().add_edge(entry_node_id.clone(), bf); }
            if let Some(bl) = body_last { *builder = builder.clone().add_loopback_edge(bl, entry_node_id.clone()); }
            Ok((Some(entry_node_id.clone()), Some(entry_node_id.clone())))
        }
    }
}

// ????????????????????????????????????????????????????
// Executor ?? ??????????
// ????????????????????????????????????????????????????

fn build_executor(
    node_id: &str,
    kind: &ExecutorKind,
    agent_resolver: &mut AgentResolver,
) -> Arc<dyn IExecutor> {
    let nid = node_id.to_string();
    match kind {
        ExecutorKind::Agent(name) => {
            match agent_resolver.get_agent(name) {
                Some(agent) => Arc::new(AgentExecutor::new(&nid, agent)),
                None => {
                    let name = name.clone();
                    Arc::new(FunctionExecutor::new(nid.clone(), move |_: String| -> Vec<String> {
                        vec![format!("[Agent '{}' not found in registry]", name)]
                    }))
                }
            }
        }

        ExecutorKind::SetVariable { variable, value } => {
            let var = variable.clone();
            let val = value.clone();
            Arc::new(ContextFunctionExecutor::new(nid.clone(), move |_msg, ctx, _prog| {
                let var = var.clone();
                let val = val.clone();
                async move { ctx.write_state(&var, val).await.map(|_| HandlerResult::None) }
            }))
        }

        ExecutorKind::SetMultipleVariables { variables } => {
            let vars = variables.clone();
            Arc::new(ContextFunctionExecutor::new(nid.clone(), move |_msg, ctx, _prog| {
                let vars = vars.clone();
                async move {
                    for (k, v) in &vars { ctx.write_state(k, v.clone()).await?; }
                    Ok(HandlerResult::None)
                }
            }))
        }

        ExecutorKind::ResetVariable { variable } => {
            let var = variable.clone();
            Arc::new(ContextFunctionExecutor::new(nid.clone(), move |_msg, ctx, _prog| {
                let var = var.clone();
                async move { ctx.clear_state(&var).await.map(|_| HandlerResult::None) }
            }))
        }

        ExecutorKind::ClearAllVariables => {
            Arc::new(ContextFunctionExecutor::new(nid.clone(), move |_msg, ctx, _prog| {
                async move {
                    let names = ctx.variable_names().await;
                    for name in &names { ctx.clear_state(name).await?; }
                    Ok(HandlerResult::None)
                }
            }))
        }

        ExecutorKind::ParseValue { source, target } => {
            let src = source.clone();
            let tgt = target.clone();
            Arc::new(ContextFunctionExecutor::new(nid.clone(), move |_msg, ctx, _prog| {
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
            Arc::new(ContextFunctionExecutor::new(nid.clone(), move |_msg, ctx, _prog| {
                let tbl = tbl.clone();
                let op = op.clone();
                let r = r.clone();
                async move {
                    let mut current = ctx.read_state(&tbl).await?.unwrap_or(serde_json::Value::Array(vec![]));
                    match op.as_str() {
                        "add" => { if let Some(arr) = current.as_array_mut() { arr.push(serde_json::to_value(&r).unwrap_or_default()); } }
                        "update" => {
                            if let Some(arr) = current.as_array_mut() {
                                for item in arr.iter_mut() {
                                    if *item == serde_json::to_value(&r).unwrap_or_default() {
                                        *item = serde_json::to_value(&r).unwrap_or_default();
                                    }
                                }
                            }
                        }
                        "delete" => {
                            if let Some(arr) = current.as_array_mut() {
                                arr.retain(|item| item != &serde_json::to_value(&r).unwrap_or_default());
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
            Arc::new(ContextFunctionExecutor::new(nid.clone(), move |_msg, ctx, _prog| {
                let txt = txt.clone();
                async move {
                    ctx.write_state("__last_activity", txt).await?;
                    Ok(HandlerResult::Messages(vec![Arc::new("activity_sent".to_string())]))
                }
            }))
        }

        ExecutorKind::ToolCall { function_name, .. } => {
            let fname = function_name.clone();
            Arc::new(FunctionExecutor::new(nid.clone(), move |_: String| -> Vec<String> {
                vec![format!("[Tool {} called]", fname)]
            }))
        }

        ExecutorKind::HumanTask(form) => {
            let f = form.clone();
            Arc::new(HumanTaskExecutor::new(&nid, Arc::new(move |_ctx| f.clone())))
        }

        ExecutorKind::HttpRequest { url, method, .. } => {
            let u = url.clone();
            let m = method.clone();
            Arc::new(FunctionExecutor::new(nid.clone(), move |_: String| -> Vec<String> {
                vec![format!("[HTTP {} {}]", m, u)]
            }))
        }

        ExecutorKind::McpRequest { server_url, tool_name, arguments, output_variable } => {
            let srv = server_url.clone();
            let tn = tool_name.clone();
            let args = arguments.clone();
            let out_var = output_variable.clone();

            // Try to look up the MCP server from the agent resolver
            if let Some(server) = agent_resolver.get_mcp_server(&srv) {
                let server = Arc::clone(server);
                Arc::new(crate::resolver::mcp_executor::McpRequestExecutor::new(
                    nid.clone(),
                    server,
                    tn,
                    args,
                    out_var,
                ))
            } else {
                // Fall back to placeholder when no MCP server is registered
                tracing::warn!(
                    server_url = %srv,
                    tool = %tn,
                    "No MCP server registered, using placeholder executor for workflow MCP tool call"
                );
                Arc::new(FunctionExecutor::new(nid.clone(), move |_: String| -> Vec<String> {
                    vec![format!("[MCP {} @ {}]", tn, srv)]
                }))
            }
        }

        ExecutorKind::EndWorkflow => {
            Arc::new(FunctionExecutor::new(nid.clone(), |_: String| -> Vec<String> { vec![] }))
        }

        ExecutorKind::CreateConversation { conversation_id } => {
            let cid = conversation_id.clone();
            Arc::new(FunctionExecutor::new(nid.clone(), move |_: String| -> Vec<String> {
                vec![format!("$create_conversation {}", cid)]
            }))
        }

        ExecutorKind::AddMessage { role, content } => {
            let r = role.clone();
            let c = content.clone();
            Arc::new(FunctionExecutor::new(nid.clone(), move |_: String| -> Vec<String> {
                vec![r.clone(), c.clone()]
            }))
        }

        ExecutorKind::NoOp => {
            Arc::new(FunctionExecutor::new(nid.clone(), |_: String| -> Vec<String> { vec![] }))
        }
    }
}

fn build_condition_executor(
    node_id: &str,
    condition: &str,
) -> Arc<dyn IExecutor> {
    let cond = condition.to_string();
    let nid = node_id.to_string();
    let nid1 = nid.clone();
    Arc::new(ContextFunctionExecutor::new(nid, move |_msg, ctx, _prog| {
        let cond = cond.clone();
        let nid1 = nid1.clone();
        async move {
            let result = evaluate_simple_condition(&cond);
            ctx.write_state(&nid1, serde_json::json!(result)).await?;
            Ok(HandlerResult::Messages(vec![Arc::new(if result { "true".to_string() } else { "false".to_string() })]))
        }
    }))
}

fn build_multi_condition_executor(
    node_id: &str,
    branches: &[(String, CompileNode)],
) -> Arc<dyn IExecutor> {
    let conditions: Vec<String> = branches.iter().map(|(c, _)| c.clone()).collect();
    let nid = node_id.to_string();
    let nid1 = nid.clone();
    let nid2 = nid.clone();
    Arc::new(ContextFunctionExecutor::new(nid.clone(), move |_msg, ctx, _prog| {
        let conditions = conditions.clone();
        let nid1 = nid1.clone();
        let nid2 = nid2.clone();
        async move {
            for (i, cond) in conditions.iter().enumerate() {
                if evaluate_simple_condition(cond) {
                    ctx.write_state(&nid1, serde_json::json!(i)).await?;
                    return Ok(HandlerResult::Messages(vec![Arc::new(i.to_string())]));
                }
            }
            ctx.write_state(&nid2, serde_json::json!(-1)).await?;
            Ok(HandlerResult::Messages(vec![Arc::new("-1".to_string())]))
        }
    }))
}

fn build_loop_entry_executor(node_id: &str) -> Arc<dyn IExecutor> {
    let nid = node_id.to_string();
    let nid2 = nid.clone();
    Arc::new(FunctionExecutor::new(nid.clone(), move |_: String| -> Vec<String> {
        vec![format!("$loop_{}", nid2)]
    }))
}

/// ?????????? ==, !=, >=, <=, >, <, contains??
///
/// ??????PowerFx ??Rhai ?????????
fn evaluate_simple_condition(expr: &str) -> bool {
    let expr = expr.trim().strip_prefix('=').unwrap_or(expr);

    // contains
    if let Some((left, right)) = expr.split_once(" contains ") {
        return left.contains(right);
    }
    // >=
    if let Some((a, b)) = expr.split_once(" >= ") {
        if let (Ok(a), Ok(b)) = (a.trim().parse::<f64>(), b.trim().parse::<f64>()) { return a >= b; }
    }
    // <=
    if let Some((a, b)) = expr.split_once(" <= ") {
        if let (Ok(a), Ok(b)) = (a.trim().parse::<f64>(), b.trim().parse::<f64>()) { return a <= b; }
    }
    // >
    if let Some((a, b)) = expr.split_once(" > ") {
        if let (Ok(a), Ok(b)) = (a.trim().parse::<f64>(), b.trim().parse::<f64>()) { return a > b; }
    }
    // <
    if let Some((a, b)) = expr.split_once(" < ") {
        if let (Ok(a), Ok(b)) = (a.trim().parse::<f64>(), b.trim().parse::<f64>()) { return a < b; }
    }
    // !=
    if let Some((a, b)) = expr.split_once(" != ") {
        return a.trim() != b.trim();
    }
    // ==
    if let Some((a, b)) = expr.split_once(" == ") {
        return a.trim() == b.trim();
    }

    match expr.to_lowercase().as_str() {
        "true" | "yes" | "1" => true,
        "false" | "no" | "0" => false,
        _ => !expr.is_empty(),
    }
}


