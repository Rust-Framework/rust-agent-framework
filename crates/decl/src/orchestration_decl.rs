use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{DeclError, Result};

/// 声明式编排模式 — 对齐 RAF `crates/workflow` 全部内置编排 + 组合 pipeline。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum OrchestrationMode {
    /// Orchestrator + FanOut 子 Agent 池（Magentic-One）
    #[default]
    Magentic,
    /// 流水线顺序执行
    Sequential,
    /// FanOut/FanIn 并行执行
    Concurrent,
    /// Triage 智能路由到专家
    Handoff,
    /// 多轮群聊讨论
    GroupChat,
    /// 多专家投票聚合
    Vote,
    /// 多阶段组合（顺序 + 并行 + 闭环）
    Pipeline,
    /// MAF ActionDecl 图（`kind: workflow` 或嵌套 trigger）
    Workflow,
    /// WorkflowBuilder 自定义图（通过 actions 编译）
    Custom,
}

/// 单条 pipeline 阶段。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "camelCase")]
pub enum PipelinePhaseDecl {
    Sequential { agents: Vec<String> },
    Concurrent { agents: Vec<String> },
}

/// 完整编排声明（`metadata.orchestration` 对象形式）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct OrchestrationDecl {
    pub mode: OrchestrationMode,
    /// Magentic / Pipeline 最大迭代次数
    #[serde(default)]
    pub max_iterations: Option<usize>,
    /// Handoff：triage 子 Agent 名称（省略则使用根 Agent 作为 triage）
    #[serde(default)]
    pub triage: Option<String>,
    /// Handoff：路由匹配字段（description / name）
    #[serde(default)]
    pub routing_field: Option<String>,
    /// GroupChat：协调者子 Agent 名称
    #[serde(default)]
    pub coordinator: Option<String>,
    /// GroupChat：最大讨论轮次
    #[serde(default)]
    pub max_rounds: Option<usize>,
    /// GroupChat：发言选择策略 — roundRobin / fixedOrder / llmCoordinator
    #[serde(default)]
    pub selector: Option<String>,
    /// GroupChat：fixedOrder 时的发言顺序（参与者索引）
    #[serde(default)]
    pub speaker_order: Vec<usize>,
    /// GroupChat：出现关键词时终止讨论
    #[serde(default)]
    pub termination_keywords: Vec<String>,
    /// Vote：majority / unanimous / weighted
    #[serde(default)]
    pub aggregator: Option<String>,
    /// Vote：加权权重（与 voters 顺序对应）
    #[serde(default)]
    pub weights: Vec<f64>,
    /// Vote：投票轮次
    #[serde(default)]
    pub voting_rounds: Option<usize>,
    /// Pipeline：阶段列表
    #[serde(default)]
    pub phases: Vec<PipelinePhaseDecl>,
    /// Pipeline：闭环时回跳到的阶段索引（0-based）
    #[serde(default)]
    pub loop_from_phase: Option<usize>,
    /// Sequential：是否将上一步输出传给下一步
    #[serde(default)]
    pub pass_output: Option<bool>,
}

impl OrchestrationDecl {
    pub fn magentic(max_iterations: usize) -> Self {
        Self {
            mode: OrchestrationMode::Magentic,
            max_iterations: Some(max_iterations),
            ..Default::default()
        }
    }

    pub fn sequential() -> Self {
        Self {
            mode: OrchestrationMode::Sequential,
            ..Default::default()
        }
    }

    pub fn concurrent() -> Self {
        Self {
            mode: OrchestrationMode::Concurrent,
            ..Default::default()
        }
    }
}

/// 从 `metadata` 解析编排声明。
///
/// 支持形式：
/// - `orchestration: magentic`（字符串简写）
/// - `orchestration: { mode: pipeline, phases: [...] }`（对象）
/// - 兼容旧版：`maxIterations` 顶层 + 有 subAgents 时默认 magentic
pub fn parse_orchestration(
    metadata: &HashMap<String, Value>,
    has_sub_agents: bool,
) -> Result<OrchestrationDecl> {
    match metadata.get("orchestration") {
        Some(Value::String(mode)) => parse_shorthand(mode, metadata),
        Some(Value::Object(obj)) => {
            let mut decl: OrchestrationDecl = serde_json::from_value(Value::Object(obj.clone()))
                .map_err(|e| DeclError::Validation(format!("Invalid orchestration object: {}", e)))?;
            hoist_legacy_metadata(metadata, &mut decl);
            Ok(decl)
        }
        None if has_sub_agents => {
            let mut decl = OrchestrationDecl::magentic(15);
            hoist_legacy_metadata(metadata, &mut decl);
            Ok(decl)
        }
        Some(other) => Err(DeclError::Validation(format!(
            "metadata.orchestration must be a string or object, got: {}",
            other
        ))),
        None => Err(DeclError::Validation(
            "metadata.orchestration is required when subAgents are declared".into(),
        )),
    }
}

fn parse_shorthand(mode: &str, metadata: &HashMap<String, Value>) -> Result<OrchestrationDecl> {
    let mode = match mode.to_ascii_lowercase().as_str() {
        "magentic" => OrchestrationMode::Magentic,
        "sequential" => OrchestrationMode::Sequential,
        "concurrent" | "parallel" => OrchestrationMode::Concurrent,
        "handoff" => OrchestrationMode::Handoff,
        "groupchat" | "group_chat" => OrchestrationMode::GroupChat,
        "vote" => OrchestrationMode::Vote,
        "pipeline" => OrchestrationMode::Pipeline,
        "workflow" => OrchestrationMode::Workflow,
        "custom" => OrchestrationMode::Custom,
        other => {
            return Err(DeclError::Validation(format!(
                "Unknown orchestration mode '{}'. Supported: magentic, sequential, concurrent, \
                 handoff, groupChat, vote, pipeline, workflow, custom",
                other
            )));
        }
    };
    let mut decl = OrchestrationDecl {
        mode,
        ..Default::default()
    };
    hoist_legacy_metadata(metadata, &mut decl);
    Ok(decl)
}

fn hoist_legacy_metadata(metadata: &HashMap<String, Value>, decl: &mut OrchestrationDecl) {
    if decl.max_iterations.is_none() {
        decl.max_iterations = metadata
            .get("maxIterations")
            .or_else(|| metadata.get("max_iterations"))
            .and_then(|v| v.as_u64())
            .map(|n| n as usize);
    }
}

/// 读取子 Agent 角色（`metadata.role`）。
pub fn sub_agent_role(metadata: &HashMap<String, Value>) -> Option<&str> {
    metadata
        .get("role")
        .and_then(|v| v.as_str())
        .or_else(|| {
            metadata
                .get("capabilityTags")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|v| v.as_str())
        })
}
