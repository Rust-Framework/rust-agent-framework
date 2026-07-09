//! 自定义执行器工厂函数。
//!
//! 提供以下执行器：
//! - `artifact_persist` — 产物持久化器，将 Agent 输出写入工作流状态 + 文件
//! - `context_inject` — 上下文注入器，从状态读取产物构建富 prompt
//! - `code_merger` — 代码合并器，FanIn 聚合并行 coder 输出
//! - `review_gateway` — 审查网关，驱动反馈循环决策

use std::path::PathBuf;
use std::sync::Arc;

use rust_agent_core::ChatMessage;
use rust_agent_workflow::{
    ContextFunctionExecutor, HandlerResult, IExecutor, IWorkflowContext, WorkflowEvent,
};

/// 从 `Arc<dyn Any>` 消息中提取 `ChatMessage` 的 assistant 文本。
///
/// AgentExecutor 产出 `ChatMessage::assistant(text)`，此辅助函数统一提取文本。
fn extract_chat_message_text(msg: &Arc<dyn std::any::Any + Send + Sync>) -> Option<String> {
    if let Some(chat_msg) = msg.downcast_ref::<ChatMessage>() {
        return Some(chat_msg.content.clone());
    }
    None
}

/// 产物持久化器 — 接收上游 Agent 输出，写入工作流状态 + 可选文件。
///
/// - `state_key`: 写入 `IWorkflowContext` 的状态键
/// - `file_path`: 可选的文件路径，同时写入文件系统
///
/// 消息透传给下游节点。
pub fn artifact_persist(
    node_id: impl Into<String>,
    state_key: &'static str,
    file_path: Option<PathBuf>,
) -> Arc<dyn IExecutor> {
    let node_id = node_id.into();
    Arc::new(ContextFunctionExecutor::new(
        node_id,
        move |msg, ctx, _progress| {
            let state_key = state_key;
            let file_path = file_path.clone();
            async move {
                // 提取 assistant 文本
                let text = extract_chat_message_text(&msg).unwrap_or_default();

                // 写入工作流共享状态
                ctx.write_state(state_key, serde_json::Value::String(text.clone()))
                    .await?;

                // 可选：写入文件系统
                if let Some(path) = &file_path {
                    if let Some(parent) = path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    std::fs::write(path, &text).map_err(|e| {
                        rust_agent_core::AgentError::WorkflowError(format!(
                            "写入产物文件失败 {:?}: {}",
                            path, e
                        ))
                    })?;
                }

                // 透传消息给下游
                Ok(HandlerResult::Messages(vec![msg]))
            }
        },
    ))
}

/// 上下文注入器 — 从工作流状态读取多个产物，构建富上下文 prompt 消息。
///
/// - `state_keys`: 要读取的状态键列表
/// - `prompt_template`: prompt 模板，`{artifacts}` 占位符被替换为读取的产物
///
/// 产出一个 `ChatMessage::user(prompt)` 传给下游 AgentExecutor。
pub fn context_inject(
    node_id: impl Into<String>,
    state_keys: Vec<&'static str>,
    prompt_template: String,
) -> Arc<dyn IExecutor> {
    let node_id = node_id.into();
    Arc::new(ContextFunctionExecutor::new(
        node_id,
        move |msg, ctx, _progress| {
            let state_keys = state_keys.clone();
            let template = prompt_template.clone();
            async move {
                // 读取所有指定状态键
                let mut artifacts = String::new();
                for key in &state_keys {
                    if let Some(val) = ctx.read_state(key).await? {
                        let text = match val {
                            serde_json::Value::String(s) => s,
                            other => other.to_string(),
                        };
                        artifacts.push_str(&format!("--- {} ---\n{}\n\n", key, text));
                    }
                }

                // 如果状态中没有产物，从输入消息提取文本作为后备
                // （用于阶段 1 入口：初始用户需求作为消息传入，尚未写入状态）
                if artifacts.is_empty() {
                    if let Some(text) = extract_chat_message_text(&msg) {
                        if !text.is_empty() {
                            artifacts.push_str(&text);
                        }
                    }
                }

                // 填充模板
                let prompt = template.replace("{artifacts}", &artifacts);
                let message = ChatMessage::user(&prompt);

                // 发出可观测性事件（不污染 runtime.outputs()，最终输出仅由 review_gateway 产生）
                ctx.emit_event(WorkflowEvent::Custom {
                    key: "context_prompt".into(),
                    data: serde_json::Value::String(prompt),
                })
                .await;

                // 传递给下游 AgentExecutor
                Ok(HandlerResult::Messages(vec![Arc::new(message)]))
            }
        },
    ))
}

/// 代码合并器 — FanIn 聚合器，合并并行 coder 的输出。
///
/// FanIn 边将多个源消息逐条投递到此节点。使用状态计数器确保只在收到
/// 全部消息后产出合并结果：
/// - 前 `expected_sources - 1` 条消息：递增计数器，返回 `None`（等待其余源）
/// - 第 `expected_sources` 条消息：从状态读取各 coder 产物，合并输出，重置计数器
///
/// `expected_sources` 必须与 FanIn 边的源节点数量一致，否则会过早触发合并
/// 或永久阻塞。依赖上游 `artifact_persist` 节点已将产物写入
/// `CODE_CHANGES_ALPHA` / `CODE_CHANGES_BETA`。
pub fn code_merger(
    node_id: impl Into<String>,
    expected_sources: usize,
) -> Arc<dyn IExecutor> {
    let node_id = node_id.into();
    Arc::new(ContextFunctionExecutor::new(
        node_id,
        move |_msg, ctx, _progress| async move {
            const MERGER_COUNT: &str = "code_merger_count";

            // 读取并递增计数器
            let count = match ctx.read_state(MERGER_COUNT).await? {
                Some(serde_json::Value::Number(n)) => n.as_u64().unwrap_or(0),
                _ => 0,
            };
            let next = count + 1;
            ctx.write_state(
                MERGER_COUNT,
                serde_json::Value::Number(serde_json::Number::from(next)),
            )
            .await?;

            if (next as usize) < expected_sources {
                // 等待其余源消息
                return Ok(HandlerResult::None);
            }

            // 全部源消息已到达，重置计数器
            ctx.write_state(
                MERGER_COUNT,
                serde_json::Value::Number(serde_json::Number::from(0)),
            )
            .await?;

            // 从状态读取两个 coder 的产物并合并（状态读取错误向上传播，缺失键视为空）
            let alpha = read_state_text(&ctx, crate::state::state_keys::CODE_CHANGES_ALPHA)
                .await?
                .unwrap_or_default();
            let beta = read_state_text(&ctx, crate::state::state_keys::CODE_CHANGES_BETA)
                .await?
                .unwrap_or_default();

            let merged = format!(
                "## coder-alpha 变更\n{}\n\n---\n\n## coder-beta 变更\n{}",
                alpha, beta
            );
            let message = ChatMessage::assistant(&merged);
            Ok(HandlerResult::Messages(vec![Arc::new(message)]))
        },
    ))
}

/// 从工作流状态读取文本值
async fn read_state_text(
    ctx: &Arc<dyn IWorkflowContext>,
    key: &str,
) -> rust_agent_core::Result<Option<String>> {
    match ctx.read_state(key).await? {
        Some(serde_json::Value::String(s)) => Ok(Some(s)),
        Some(other) => Ok(Some(other.to_string())),
        None => Ok(None),
    }
}

/// 审查网关 — 反馈循环的核心决策节点。
///
/// 接收上游 reviewer 的审查结论消息，解析 `ReviewVerdict`：
/// - `passed=true` → 调用 `ctx.yield_output` 产生工作流输出，返回 `HandlerResult::None`
///   （消息不沿出边路由，工作流自然完成）
/// - `passed=false` → 产出修复提示消息，返回 `HandlerResult::Messages`
///   （消息沿 `add_loopback_edge` 回到 p4a_inject 继续循环）
///
/// 此设计避免了框架不支持"条件回边"的限制：网关本身根据审查结果决定是
/// 产生输出（终止循环）还是沿回边继续循环。配合 `LoopOptions::new(3)`
/// 限制最大迭代次数，形成双重保护。
pub fn review_gateway(node_id: impl Into<String>) -> Arc<dyn IExecutor> {
    let node_id = node_id.into();
    Arc::new(ContextFunctionExecutor::new(
        node_id,
        move |msg, ctx, _progress| async move {
            // 优先从消息中提取审查结论，回退到状态读取
            let feedback = if let Some(text) = extract_chat_message_text(&msg) {
                text
            } else {
                read_state_text(&ctx, crate::state::state_keys::REVIEW_FEEDBACK)
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or_default()
            };

            match crate::state::ReviewVerdict::parse_from_text(&feedback) {
                Some(verdict) if verdict.passed => {
                    tracing::info!("review_gateway: 审查通过，产生工作流输出，循环终止");
                    // 产生工作流输出
                    ctx.yield_output(msg.clone()).await?;
                    // 不沿出边路由，工作流自然完成
                    Ok(HandlerResult::None)
                }
                Some(verdict) => {
                    tracing::info!(
                        "review_gateway: 审查未通过（{} 个差异），继续循环",
                        verdict.discrepancies.len()
                    );
                    let prompt = format!("上一轮审查未通过，请根据以下反馈修复：\n\n{}", feedback);
                    let message = ChatMessage::user(&prompt);
                    Ok(HandlerResult::Messages(vec![Arc::new(message)]))
                }
                None => {
                    tracing::warn!("review_gateway: 无法解析 ReviewVerdict，默认继续循环");
                    let message = ChatMessage::user("审查结论解析失败，请重新评估并修复");
                    Ok(HandlerResult::Messages(vec![Arc::new(message)]))
                }
            }
        },
    ))
}
