//! 自定义执行器工厂函数。
//!
//! 提供以下执行器：
//! - `artifact_persist` — 产物持久化器，将 Agent 输出写入工作流状态 + 文件
//! - `context_inject` — 上下文注入器，从状态读取产物构建富 prompt
//! - `code_merger` — FanIn 栅栏，等待并行 coder 完成后向下游放行
//! - `review_gateway` — 审查网关，驱动反馈循环决策
//! - `loop_reset` — 循环重置器，在反馈循环回边入口清理上一轮副作用

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

/// 代码合并器 — FanIn 栅栏，等待所有并行 coder 完成后向下游放行。
///
/// coder 通过受限的 `WriteFile`/`EditFile` 工具直接在工作区写真实代码文件，
/// 因此本节点不再做文本拼接——它仅作为同步点：等待 `expected_sources` 条
/// 消息全部到达后，产出一条"并行编码完成"的汇总消息（包含各 coder 自述的
/// 变更清单，从状态读取），供回归测试器参考。
///
/// `expected_sources` 必须与 FanIn 边的源节点数量一致，否则会过早放行
/// 或永久阻塞。
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

            // 代码已由 coder 工具直接写入工作区文件，此处仅汇总各 coder 自述的
            // 变更清单（状态读取错误向上传播，缺失键视为空）。
            let alpha = read_state_text(&ctx, crate::state::state_keys::CODE_CHANGES_ALPHA)
                .await?
                .unwrap_or_default();
            let beta = read_state_text(&ctx, crate::state::state_keys::CODE_CHANGES_BETA)
                .await?
                .unwrap_or_default();

            let summary = format!(
                "## coder-alpha 变更清单\n{}\n\n---\n\n## coder-beta 变更清单\n{}\n\n---\n\n\
                 上述两路代码已直接写入工作区。请回归测试器扫描工作区并对真实落盘的代码执行测试。",
                alpha, beta
            );
            let message = ChatMessage::assistant(&summary);
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

/// 循环重置器 — 在反馈循环回边入口清理上一轮的副作用。
///
/// 当 `review_gateway` 判定未通过、消息沿回边回到 p4a_inject 时，本节点
/// 负责重置循环相关的 workflow 状态键（`code_merger_count`、
/// `CODE_CHANGES_ALPHA/BETA`、`REGRESSION_RESULTS`、`REVIEW_FEEDBACK`），
/// 并尝试 `git restore` 回滚上一轮 coder 写入工作区的未提交代码变更。
///
/// `git restore` 失败不阻断工作流（非 git 工作区或无变更时静默跳过），
/// 仅记录警告。状态重置是权威清理路径，确保下一轮 coder 从干净状态开始。
///
/// 透传输入消息给下游。
pub fn loop_reset(
    node_id: impl Into<String>,
    workspace_root: PathBuf,
) -> Arc<dyn IExecutor> {
    let node_id = node_id.into();
    Arc::new(ContextFunctionExecutor::new(
        node_id,
        move |msg, ctx, _progress| {
            let workspace_root = workspace_root.clone();
            async move {
                const RESET_KEYS: &[&str] = &[
                    "code_merger_count",
                    crate::state::state_keys::CODE_CHANGES_ALPHA,
                    crate::state::state_keys::CODE_CHANGES_BETA,
                    crate::state::state_keys::REGRESSION_RESULTS,
                    crate::state::state_keys::REVIEW_FEEDBACK,
                ];

                for key in RESET_KEYS {
                    ctx.write_state(*key, serde_json::Value::Null).await?;
                }

                ctx.emit_event(WorkflowEvent::Custom {
                    key: "loop_reset".into(),
                    data: serde_json::json!({ "workspace": workspace_root.display().to_string() }),
                })
                .await;

                // 尝试 git restore 回滚未提交的代码变更（非 git 工作区静默跳过）
                match std::process::Command::new("git")
                    .arg("restore")
                    .arg(":/:")
                    .current_dir(&workspace_root)
                    .output()
                {
                    Ok(out) if !out.status.success() => {
                        tracing::warn!(
                            "loop_reset: git restore 未成功（可能是非 git 工作区或无变更）: {}",
                            String::from_utf8_lossy(&out.stderr).trim()
                        );
                    }
                    Err(e) => {
                        tracing::warn!("loop_reset: git 不可用，跳过磁盘回滚: {}", e);
                    }
                    _ => {
                        tracing::info!("loop_reset: 已回滚工作区未提交变更");
                    }
                }

                // 透传消息给下游（p4a_inject）
                Ok(HandlerResult::Messages(vec![msg]))
            }
        },
    ))
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
