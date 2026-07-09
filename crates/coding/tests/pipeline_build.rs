//! 测试工作流图构建 — 验证 6 阶段流水线图结构正确。

use rust_agent_client::ChatClientOptions;
use rust_agent_coding::build_dev_pipeline;

fn mock_options() -> ChatClientOptions {
    ChatClientOptions {
        api_base: "https://api.deepseek.com/v1".into(),
        api_key: "mock-key-for-testing".into(),
        model: "deepseek-chat".into(),
        ..Default::default()
    }
}

#[test]
fn test_pipeline_builds_successfully() {
    let options = mock_options();
    let workspace = tempfile::tempdir().expect("tempdir");
    let graph = build_dev_pipeline(&options, workspace.path()).expect("build pipeline");

    // 验证入口节点
    assert_eq!(graph.start_node_id(), "p1_inject");
}

#[test]
fn test_pipeline_has_all_stage_nodes() {
    let options = mock_options();
    let workspace = tempfile::tempdir().expect("tempdir");
    let graph = build_dev_pipeline(&options, workspace.path()).expect("build pipeline");

    let nodes = graph.nodes();

    // 阶段 1: 需求分析 + HITL
    assert!(nodes.contains_key("p1_inject"), "缺少 p1_inject");
    assert!(nodes.contains_key("p1_analyst"), "缺少 p1_analyst");
    assert!(nodes.contains_key("p1_persist"), "缺少 p1_persist");
    assert!(nodes.contains_key("p1_confirm"), "缺少 p1_confirm");

    // 阶段 2: 测试驱动设计
    assert!(nodes.contains_key("p2_inject"), "缺少 p2_inject");
    assert!(nodes.contains_key("p2_designer"), "缺少 p2_designer");
    assert!(nodes.contains_key("p2_persist"), "缺少 p2_persist");

    // 阶段 3: 架构设计
    assert!(nodes.contains_key("p3_inject"), "缺少 p3_inject");
    assert!(nodes.contains_key("p3_architect"), "缺少 p3_architect");
    assert!(nodes.contains_key("p3_persist"), "缺少 p3_persist");

    // 阶段 4a: 任务分解
    assert!(nodes.contains_key("p4a_inject"), "缺少 p4a_inject");
    assert!(nodes.contains_key("p4a_planner"), "缺少 p4a_planner");
    assert!(nodes.contains_key("p4a_persist"), "缺少 p4a_persist");

    // 阶段 4b: 并行编码
    assert!(
        nodes.contains_key("p4b_alpha_inject"),
        "缺少 p4b_alpha_inject"
    );
    assert!(
        nodes.contains_key("p4b_alpha_coder"),
        "缺少 p4b_alpha_coder"
    );
    assert!(
        nodes.contains_key("p4b_alpha_persist"),
        "缺少 p4b_alpha_persist"
    );
    assert!(
        nodes.contains_key("p4b_beta_inject"),
        "缺少 p4b_beta_inject"
    );
    assert!(nodes.contains_key("p4b_beta_coder"), "缺少 p4b_beta_coder");
    assert!(
        nodes.contains_key("p4b_beta_persist"),
        "缺少 p4b_beta_persist"
    );
    assert!(nodes.contains_key("p4b_merger"), "缺少 p4b_merger");

    // 阶段 5: 回归测试
    assert!(nodes.contains_key("p5_inject"), "缺少 p5_inject");
    assert!(nodes.contains_key("p5_tester"), "缺少 p5_tester");
    assert!(nodes.contains_key("p5_persist"), "缺少 p5_persist");

    // 阶段 6: 审查与反馈循环
    assert!(nodes.contains_key("p6_inject"), "缺少 p6_inject");
    assert!(nodes.contains_key("p6_reviewer"), "缺少 p6_reviewer");
    assert!(nodes.contains_key("p6_persist"), "缺少 p6_persist");
    assert!(nodes.contains_key("p6_gateway"), "缺少 p6_gateway");
}

#[test]
fn test_pipeline_has_loop_options_on_p4a_inject() {
    let options = mock_options();
    let workspace = tempfile::tempdir().expect("tempdir");
    let graph = build_dev_pipeline(&options, workspace.path()).expect("build pipeline");

    let node = graph.get_node("p4a_inject").expect("p4a_inject exists");
    let loop_options = node
        .loop_options
        .as_ref()
        .expect("p4a_inject 应有 LoopOptions");
    assert_eq!(loop_options.max_iterations, 3, "p4a_inject 最多迭代 3 次");
}
