//! 6 阶段开发流水线工作流图构建器。
//!
//! 遵循"以终为始"哲学：
//! - 需求分析（含 HITL 确认）→ 测试驱动设计 → 架构设计
//! - 并行编码（FanOut/FanIn）→ 回归测试 → 审查与反馈循环
//!
//! 反馈循环采用 `review_gateway` 智能网关模式（详见 `executors::review_gateway`）：
//! 网关根据审查结果决定是产生工作流输出（终止）还是沿回边继续循环。

use std::path::Path;
use std::sync::Arc;

use rust_agent_client::ChatClientOptions;
use rust_agent_core::Result;
use rust_agent_workflow::{
    AgentExecutor, HumanTaskExecutor, LoopOptions, WorkflowBuilder, WorkflowGraph,
};

use crate::agents::{
    create_architect, create_coder, create_regression_tester, create_requirements_analyst,
    create_reviewer, create_task_planner, create_test_designer,
};
use crate::executors::{artifact_persist, code_merger, context_inject, loop_reset, review_gateway};
use crate::state::state_keys;

/// 构建完整的 6 阶段开发流水线工作流图。
///
/// # 参数
/// - `options`: LLM 客户端配置（每个 Agent 独立创建客户端）
/// - `workspace_root`: 工作区根目录（供 Agent 文件工具使用）
///
/// # 返回
/// 已通过校验的 `WorkflowGraph`，可直接用于 `WorkflowRuntime::start`。
///
/// # 图结构
/// ```text
/// p1_inject → p1_analyst → p1_persist → p1_confirm (HITL)
///   → p2_inject → p2_designer → p2_persist
///   → p3_inject → p3_architect → p3_persist
///   → p4a_loop_reset → p4a_inject [LoopOptions:3] → p4a_planner → p4a_persist
///     → FanOut(p4b_alpha_*, p4b_beta_*) → FanIn(p4b_merger)
///   → p5_inject → p5_tester → p5_persist
///   → p6_inject → p6_reviewer → p6_persist → p6_gateway (review_gateway)
///     ├── 审查通过 → yield_output (工作流完成)
///     └── 审查未通过 → 回边 → p4a_loop_reset → p4a_inject (清理后继续循环)
/// ```
pub fn build_dev_pipeline(
    options: &ChatClientOptions,
    workspace_root: &Path,
) -> Result<WorkflowGraph> {
    // ── 阶段 1: 需求分析 + HITL 确认 ──────────────────────────────
    let p1_analyst = create_requirements_analyst(options, workspace_root)?;
    let p1_inject = context_inject(
        "p1_inject",
        vec![],
        "你是需求分析专家。请根据以下用户需求进行全面的需求分解：\n\n{artifacts}\n\n（如果上方为空，请基于初始消息分析）".to_string(),
    );
    let p1_persist = artifact_persist(
        "p1_persist",
        state_keys::REQUIREMENTS_DOC,
        Some(workspace_root.join(".coding").join("requirements.md")),
    );
    let p1_confirm = Arc::new(HumanTaskExecutor::new(
        "p1_confirm",
        Arc::new(|_ctx| {
            serde_json::json!({
                "task": "需求确认",
                "instruction": "请审查以上需求分析文档。回复 \"确认\" 以继续，或提供修改建议。",
                "stage": "requirements_analysis"
            })
        }),
    ));

    // ── 阶段 2: 测试驱动设计 ──────────────────────────────────────
    let p2_designer = create_test_designer(options, workspace_root)?;
    let p2_inject = context_inject(
        "p2_inject",
        vec![state_keys::REQUIREMENTS_DOC],
        "根据以下需求文档，编写完整的集成测试用例和冒烟测试用例。测试用例应验证最终交付结果的形态：\n\n{artifacts}".to_string(),
    );
    let p2_persist = artifact_persist(
        "p2_persist",
        state_keys::TEST_CASES,
        Some(workspace_root.join(".coding").join("test_cases.md")),
    );

    // ── 阶段 3: 架构设计 ──────────────────────────────────────────
    let p3_architect = create_architect(options, workspace_root)?;
    let p3_inject = context_inject(
        "p3_inject",
        vec![state_keys::REQUIREMENTS_DOC, state_keys::TEST_CASES],
        "基于以下需求文档和测试用例，设计软件架构。明确项目结构、文件分布、代码职责、扩展点：\n\n{artifacts}".to_string(),
    );
    let p3_persist = artifact_persist(
        "p3_persist",
        state_keys::ARCHITECTURE_DOC,
        Some(workspace_root.join(".coding").join("architecture.md")),
    );

    // ── 阶段 4a: 任务分解（回环入口，LoopOptions 限制 3 次迭代）─────
    let p4a_planner = create_task_planner(options, workspace_root)?;
    let p4a_loop_reset = loop_reset("p4a_loop_reset", workspace_root.to_path_buf());
    let p4a_inject = context_inject(
        "p4a_inject",
        vec![
            state_keys::REQUIREMENTS_DOC,
            state_keys::TEST_CASES,
            state_keys::ARCHITECTURE_DOC,
            state_keys::REVIEW_FEEDBACK,
        ],
        "基于以下需求、测试、架构（及上一轮审查反馈，如有），分解开发任务。遵循高内聚低耦合原则，拆分可并行编码内容：\n\n{artifacts}".to_string(),
    );
    let p4a_persist = artifact_persist(
        "p4a_persist",
        state_keys::TASK_PLAN,
        Some(workspace_root.join(".coding").join("task_plan.md")),
    );

    // ── 阶段 4b: 并行编码（FanOut → alpha/beta coder → FanIn 合并）──
    let p4b_alpha_coder = create_coder(options, workspace_root, "coder-alpha")?;
    let p4b_beta_coder = create_coder(options, workspace_root, "coder-beta")?;
    let p4b_alpha_inject = context_inject(
        "p4b_alpha_inject",
        vec![state_keys::TASK_PLAN, state_keys::ARCHITECTURE_DOC],
        "你是 coder-alpha。根据以下任务计划和架构，实现分配给你的模块（优先处理核心逻辑层）：\n\n{artifacts}".to_string(),
    );
    let p4b_beta_inject = context_inject(
        "p4b_beta_inject",
        vec![state_keys::TASK_PLAN, state_keys::ARCHITECTURE_DOC],
        "你是 coder-beta。根据以下任务计划和架构，实现分配给你的模块（优先处理接口/适配层）：\n\n{artifacts}".to_string(),
    );
    let p4b_alpha_persist =
        artifact_persist("p4b_alpha_persist", state_keys::CODE_CHANGES_ALPHA, None);
    let p4b_beta_persist =
        artifact_persist("p4b_beta_persist", state_keys::CODE_CHANGES_BETA, None);
    let p4b_merger = code_merger("p4b_merger", 2);

    // ── 阶段 5: 回归测试 ──────────────────────────────────────────
    let p5_tester = create_regression_tester(options, workspace_root)?;
    let p5_inject = context_inject(
        "p5_inject",
        vec![
            state_keys::TASK_PLAN,
            state_keys::CODE_CHANGES_ALPHA,
            state_keys::CODE_CHANGES_BETA,
            state_keys::TEST_CASES,
        ],
        "根据以下任务计划、代码变更和测试用例，执行回归测试。对照集成测试和冒烟测试预期结果：\n\n{artifacts}".to_string(),
    );
    let p5_persist = artifact_persist(
        "p5_persist",
        state_keys::REGRESSION_RESULTS,
        Some(workspace_root.join(".coding").join("regression.md")),
    );

    // ── 阶段 6: 审查与反馈循环 ────────────────────────────────────
    let p6_reviewer = create_reviewer(options, workspace_root)?;
    let p6_inject = context_inject(
        "p6_inject",
        vec![
            state_keys::REQUIREMENTS_DOC,
            state_keys::TEST_CASES,
            state_keys::REGRESSION_RESULTS,
        ],
        "你是审查专家。对照以下需求、测试用例和回归结果，审查全链路是否达成预期。必须输出 JSON 格式的审查结论：\n\
         {\"passed\": bool, \"discrepancies\": [string], \"root_cause\": string, \"fix_suggestions\": [string]}\n\n{artifacts}".to_string(),
    );
    let p6_persist = artifact_persist(
        "p6_persist",
        state_keys::REVIEW_FEEDBACK,
        Some(workspace_root.join(".coding").join("review.md")),
    );
    // review_gateway: 审查通过→yield_output(工作流完成)，审查未通过→沿回边继续循环
    let p6_gateway = review_gateway("p6_gateway");

    // ── 构建工作流图 ──────────────────────────────────────────────
    let builder = WorkflowBuilder::new()
        // 阶段 1
        .add_node("p1_inject", p1_inject)
        .add_node(
            "p1_analyst",
            Arc::new(AgentExecutor::new("p1_analyst", p1_analyst)),
        )
        .add_node("p1_persist", p1_persist)
        .add_node("p1_confirm", p1_confirm)
        // 阶段 2
        .add_node("p2_inject", p2_inject)
        .add_node(
            "p2_designer",
            Arc::new(AgentExecutor::new("p2_designer", p2_designer)),
        )
        .add_node("p2_persist", p2_persist)
        // 阶段 3
        .add_node("p3_inject", p3_inject)
        .add_node(
            "p3_architect",
            Arc::new(AgentExecutor::new("p3_architect", p3_architect)),
        )
        .add_node("p3_persist", p3_persist)
        // 阶段 4a
        .add_node("p4a_loop_reset", p4a_loop_reset)
        .add_node("p4a_inject", p4a_inject)
        .add_node(
            "p4a_planner",
            Arc::new(AgentExecutor::new("p4a_planner", p4a_planner)),
        )
        .add_node("p4a_persist", p4a_persist)
        // 阶段 4b — 并行编码
        .add_node("p4b_alpha_inject", p4b_alpha_inject)
        .add_node(
            "p4b_alpha_coder",
            Arc::new(AgentExecutor::new("p4b_alpha_coder", p4b_alpha_coder)),
        )
        .add_node("p4b_alpha_persist", p4b_alpha_persist)
        .add_node("p4b_beta_inject", p4b_beta_inject)
        .add_node(
            "p4b_beta_coder",
            Arc::new(AgentExecutor::new("p4b_beta_coder", p4b_beta_coder)),
        )
        .add_node("p4b_beta_persist", p4b_beta_persist)
        .add_node("p4b_merger", p4b_merger)
        // 阶段 5
        .add_node("p5_inject", p5_inject)
        .add_node(
            "p5_tester",
            Arc::new(AgentExecutor::new("p5_tester", p5_tester)),
        )
        .add_node("p5_persist", p5_persist)
        // 阶段 6
        .add_node("p6_inject", p6_inject)
        .add_node(
            "p6_reviewer",
            Arc::new(AgentExecutor::new("p6_reviewer", p6_reviewer)),
        )
        .add_node("p6_persist", p6_persist)
        .add_node("p6_gateway", p6_gateway)
        // 入口（output 由 review_gateway 通过 yield_output 产生）
        .set_start("p1_inject")
        // 循环限制：p4a_inject 最多迭代 3 次
        .with_loop_on("p4a_inject", LoopOptions::new(3))
        // 阶段 1 边
        .add_edge("p1_inject", "p1_analyst")
        .add_edge("p1_analyst", "p1_persist")
        .add_edge("p1_persist", "p1_confirm")
        .add_edge("p1_confirm", "p2_inject")
        // 阶段 2 边
        .add_edge("p2_inject", "p2_designer")
        .add_edge("p2_designer", "p2_persist")
        .add_edge("p2_persist", "p3_inject")
        // 阶段 3 边
        .add_edge("p3_inject", "p3_architect")
        .add_edge("p3_architect", "p3_persist")
        .add_edge("p3_persist", "p4a_loop_reset")
        // 阶段 4a 边
        .add_edge("p4a_loop_reset", "p4a_inject")
        .add_edge("p4a_inject", "p4a_planner")
        .add_edge("p4a_planner", "p4a_persist")
        // FanOut: p4a_persist → alpha/beta 并行
        .add_fan_out_edge("p4a_persist", vec!["p4b_alpha_inject", "p4b_beta_inject"])
        .add_edge("p4b_alpha_inject", "p4b_alpha_coder")
        .add_edge("p4b_alpha_coder", "p4b_alpha_persist")
        .add_edge("p4b_beta_inject", "p4b_beta_coder")
        .add_edge("p4b_beta_coder", "p4b_beta_persist")
        // FanIn: alpha/beta persist → merger
        .add_fan_in_edge(vec!["p4b_alpha_persist", "p4b_beta_persist"], "p4b_merger")
        // 阶段 5 边
        .add_edge("p4b_merger", "p5_inject")
        .add_edge("p5_inject", "p5_tester")
        .add_edge("p5_tester", "p5_persist")
        // 阶段 6 边
        .add_edge("p5_persist", "p6_inject")
        .add_edge("p6_inject", "p6_reviewer")
        .add_edge("p6_reviewer", "p6_persist")
        .add_edge("p6_persist", "p6_gateway")
        // 反馈循环：审查未通过时消息沿回边回到 p4a_loop_reset（清理后进入 p4a_inject）
        .add_loopback_edge("p6_gateway", "p4a_loop_reset");

    builder.build()
}
