# 开发流水线编排 — 实现计划

## 摘要

基于 RAF 框架实现 6 阶段自动化软件开发智能体编排（以终为始哲学），交付为独立 crate `rust-agent-coding`。当前 crate 已有 4 个源文件（state.rs / conditions.rs / agents.rs / executors.rs）和 Cargo.toml，但缺少 lib.rs / pipeline.rs / bin/coding.rs 导致无法编译。本计划完成剩余实现，并解决反馈循环网关的设计问题。

## 当前状态分析

### 已完成
- `crates/coding/Cargo.toml` — 包配置、依赖、bin 声明（但 bin 文件缺失）
- `crates/coding/src/state.rs` — 10 个状态键常量 + `ReviewVerdict` 结构 + JSON 解析 + 单元测试
- `crates/coding/src/conditions.rs` — `ReviewPassedCondition` 实现 `IEdgeCondition`
- `crates/coding/src/agents.rs` — 7 个 Agent 工厂函数（requirements-analyst / test-designer / architect / task-planner / coder / regression-tester / reviewer）
- `crates/coding/src/executors.rs` — `artifact_persist` / `context_inject` / `code_merger` / `pass_through` / `pass_through_string`
- 根 `Cargo.toml` — 已注册 `crates/coding` 到 members 和 workspace dependencies

### 缺失（导致无法编译）
- `src/lib.rs` — crate 根，模块声明
- `src/pipeline.rs` — 工作流图构建器
- `src/bin/coding.rs` — 二进制入口（Cargo.toml 已声明）
- 测试文件

### 关键设计问题：反馈循环网关

**框架限制**（经深入探索确认）：
- `add_loopback_edge(source, target)` — 无条件回边（`condition=None`, `is_loopback=true`）
- `add_edge_with_condition(source, target, cond)` — 条件边（`condition=Some`, `is_loopback=false`）
- **不存在"条件回边"API**（无法同时设置 `is_loopback=true` 和 `condition=Some`）
- 多出边采用**广播式路由**：所有出边独立评估 condition，不互斥
- Executor 无法控制路由方向（`send_message` 仍走边路由）
- 回边循环终止依赖 `LoopConfig.max_iterations`

**解决方案：feedback_filter 吞没模式**

利用广播式路由 + ContextFunctionExecutor 的 `HandlerResult::None` 能力：

```
p6_persist → p6_gateway (pass_through)
                ├── add_edge_with_condition → output  (ReviewPassedCondition)
                └── add_loopback_edge        → feedback_filter
                                              ↓
                                   (passed=true → None 吞没)
                                   (passed=false → 产出消息 → p4a_inject 回环)
```

- **审查通过**：消息广播到 output（流程完成）和 feedback_filter（吞没，不继续循环）
- **审查未通过**：条件边不投递，只有回边投递到 feedback_filter，产出消息继续循环
- **额外保护**：p4a_inject 设置 `LoopConfig::new(3)` 限制最大迭代次数

## 提议变更

### 1. 修改 `src/executors.rs` — 新增 `feedback_filter` 执行器

**为什么**：反馈循环网关需要根据审查结论决定是否继续循环。由于框架不支持条件回边，使用 ContextFunctionExecutor 在回边目标前过滤消息。

**做什么**：新增 `feedback_filter` 工厂函数，读取 `REVIEW_FEEDBACK` 状态，解析 `ReviewVerdict`：
- `passed=true` → 返回 `HandlerResult::None`（吞没消息，循环终止）
- `passed=false` → 产出 `ChatMessage::user("审查未通过，继续修复")` → 走到 p4a_inject

**怎么做**：
```rust
pub fn feedback_filter(node_id: impl Into<String>) -> Arc<dyn IExecutor> {
    // ContextFunctionExecutor
    // 1. read_state(REVIEW_FEEDBACK)
    // 2. ReviewVerdict::parse_from_text(&text)
    // 3. if verdict.passed { Ok(HandlerResult::None) }
    //    else { Ok(HandlerResult::Messages(vec![ChatMessage::user("继续修复")])) }
}
```

### 2. 修改 `src/conditions.rs` — 更新文档注释

**为什么**：原注释提到 `exclusive_gateway`，但实际方案使用 `add_edge_with_condition` + `add_loopback_edge` 双边模式。

**做什么**：更新模块和结构体文档注释，反映 feedback_filter 方案。

### 3. 新建 `src/pipeline.rs` — 工作流图构建器

**为什么**：将 7 个 Agent + 执行器 + 边路由组装为完整的 6 阶段 DAG。

**做什么**：定义 `DevPipeline` 结构和 `build_dev_pipeline(options, workspace_root) -> Result<WorkflowGraph>` 函数。

**图结构**：
```
start (set_start)
  ↓
p1_inject (context_inject: 初始需求占位)
  ↓
p1_analyst (AgentExecutor: requirements-analyst)
  ↓
p1_persist (artifact_persist: REQUIREMENTS_DOC)
  ↓
p1_confirm (HumanTaskExecutor: 用户确认)
  ↓
p2_inject (context_inject: REQUIREMENTS_DOC)
  ↓
p2_designer (AgentExecutor: test-designer)
  ↓
p2_persist (artifact_persist: TEST_CASES)
  ↓
p3_inject (context_inject: REQUIREMENTS_DOC + TEST_CASES)
  ↓
p3_architect (AgentExecutor: architect)
  ↓
p3_persist (artifact_persist: ARCHITECTURE_DOC)
  ↓
p4a_inject (context_inject: REQUIREMENTS_DOC + TEST_CASES + ARCHITECTURE_DOC + REVIEW_FEEDBACK)
           [with_loop_on: LoopConfig::new(3)]
  ↓
p4a_planner (AgentExecutor: task-planner)
  ↓
p4a_persist (artifact_persist: TASK_PLAN)
  ↓ FanOut
  ├── p4b_alpha_inject → p4b_alpha_coder → p4b_alpha_persist (CODE_CHANGES_ALPHA)
  └── p4b_beta_inject  → p4b_beta_coder  → p4b_beta_persist  (CODE_CHANGES_BETA)
       ↓ FanIn
       p4b_merger (code_merger)
  ↓
p5_inject (context_inject: TASK_PLAN + CODE_CHANGES_ALPHA + CODE_CHANGES_BETA + TEST_CASES)
  ↓
p5_tester (AgentExecutor: regression-tester)
  ↓
p5_persist (artifact_persist: REGRESSION_RESULTS)
  ↓
p6_inject (context_inject: REQUIREMENTS_DOC + TEST_CASES + REGRESSION_RESULTS)
  ↓
p6_reviewer (AgentExecutor: reviewer)
  ↓
p6_persist (artifact_persist: REVIEW_FEEDBACK)
  ↓
p6_gateway (pass_through)
  ├── add_edge_with_condition(ReviewPassedCondition) → output
  └── add_loopback_edge → feedback_filter
                           ↓ (passed=false 时产出)
                           p4a_inject (回环)
```

**关键实现细节**：
- 使用 `WorkflowBuilder::new()` 链式构建
- `set_start("p1_inject")`
- `with_output_from("p6_gateway")` — 注意：output 节点是 gateway，通过条件边投递的消息成为工作流输出
- FanOut: `add_fan_out_edge("p4a_persist", vec!["p4b_alpha_inject", "p4b_beta_inject"])`
- FanIn: `add_fan_in_edge(vec!["p4b_alpha_persist", "p4b_beta_persist"], "p4b_merger")`
- 回环: `add_loopback_edge("p6_gateway", "feedback_filter")` + `add_edge("feedback_filter", "p4a_inject")`
- 循环限制: `with_loop_on("p4a_inject", LoopConfig::new(3))`
- AgentExecutor 包装: `Arc::new(AgentExecutor::new(agent))`

**HumanTaskExecutor 配置**：
```rust
Arc::new(HumanTaskExecutor::new(
    "p1_confirm",
    Arc::new(|ctx| {
        // 从状态读取需求文档，构造确认表单
        // 注意：闭包内无法直接 await，需要同步读取
        // 实际实现：返回一个包含指令的 JSON，由外部消费者处理
        serde_json::json!({
            "task": "请确认需求文档",
            "instruction": "审查以上需求分析，回复确认或修改建议",
            "state_key": "requirements_doc"
        })
    }),
))
```

注意：`HumanTaskExecutor::new` 的 `task_builder` 闭包签名是 `Fn(&dyn IWorkflowContext) -> serde_json::Value`（同步），无法在闭包内 `await` 读取状态。因此 task_builder 返回静态表单结构，实际需求文档已通过 `yield_output` 在 p1_persist 阶段输出给消费者。

### 4. 新建 `src/lib.rs` — crate 根

**为什么**：组织模块树，公开 API。

**做什么**：
```rust
pub mod agents;
pub mod conditions;
pub mod executors;
pub mod pipeline;
pub mod state;

// 重导出常用类型
pub use agents::{
    create_architect, create_coder, create_regression_tester, create_requirements_analyst,
    create_reviewer, create_task_planner, create_test_designer,
};
pub use conditions::ReviewPassedCondition;
pub use executors::{artifact_persist, code_merger, context_inject, feedback_filter, pass_through};
pub use pipeline::build_dev_pipeline;
pub use state::{state_keys, ReviewVerdict};
```

### 5. 新建 `src/bin/coding.rs` — 交互式二进制入口

**为什么**：提供 CLI 入口，演示完整 HITL 流程。

**做什么**：
1. 解析命令行参数（API key、模型、初始需求）
2. 构建 `ChatClientOptions`
3. 调用 `build_dev_pipeline` 构建图
4. `WorkflowRuntime::start(graph, initial_message, None)` 启动
5. 监听 `runtime.events()`：
   - `WorkflowHalted` → 从 payload 读取任务，提示用户输入，调用 `runtime.resume(ResumeCommand::InjectMessage { target_node_id: "p1_confirm", message: Arc::new(user_input) })`
   - `WorkflowCompleted` → 退出
6. 可选：消费 `runtime.outputs()` 打印中间产物

**关键代码结构**：
```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. 解析参数
    let api_key = std::env::var("DEEPSEEK_API_KEY")?;
    let options = ChatClientOptions { /* ... */ };
    let workspace_root = std::env::current_dir()?;
    let initial_requirement = std::env::args().nth(1).unwrap_or_default();

    // 2. 构建图
    let graph = build_dev_pipeline(&options, &workspace_root)?;

    // 3. 启动
    let runtime = WorkflowRuntime::start(
        graph,
        Arc::new(ChatMessage::user(&initial_requirement)),
        None,
    ).await?;

    // 4. 事件循环
    let mut events = runtime.events().await.expect("events");
    while let Some(ev) = events.next().await {
        match ev {
            WorkflowEvent::WorkflowHalted { node_id, payload, .. } => {
                // 提示用户确认
                println!("需求分析完成，请审查并确认...");
                let user_input = read_user_input()?;
                runtime.resume(ResumeCommand::InjectMessage {
                    target_node_id: node_id,
                    message: Arc::new(user_input),
                })?;
            }
            WorkflowEvent::WorkflowCompleted { .. } => break,
            WorkflowEvent::WorkflowError(e) => return Err(e.into()),
            _ => {}
        }
    }

    runtime.wait().await?;
    Ok(())
}
```

### 6. 新建测试文件

#### `tests/pipeline_build.rs`
- 测试 `build_dev_pipeline` 返回的 `WorkflowGraph` 能成功构建
- 验证关键节点存在（p1_inject, p1_confirm, p4a_inject, p4b_merger, p6_gateway, feedback_filter）
- 验证入口节点为 p1_inject
- 验证 output 节点为 p6_gateway
- 使用 mock ChatClientOptions（不需要真实 API key，因为只测试图结构）

#### `tests/hitl_confirm.rs`
- 测试 HumanTaskExecutor 暂停与恢复
- 构建最小图：p1_inject → p1_confirm → output
- 启动 runtime，监听 WorkflowHalted
- 调用 resume 注入确认消息
- 验证 WorkflowCompleted

#### `tests/parallel_coding.rs`
- 测试 FanOut/FanIn 并行编码
- 构建最小图：entry → FanOut(alpha, beta) → FanIn(merger) → output
- 使用 FunctionExecutor 替代真实 Agent（避免 API 调用）
- 验证 merger 正确合并两条消息

#### `tests/feedback_loop.rs`
- 测试反馈循环网关
- 构建最小图：reviewer_persist → gateway → (条件边→output, 回边→feedback_filter→loop_target)
- 手动设置 REVIEW_FEEDBACK 状态为 passed=true，验证消息到达 output
- 手动设置 REVIEW_FEEDBACK 状态为 passed=false，验证消息回环到 loop_target
- 验证 LoopConfig 限制最大迭代次数

## 假设与决策

### 决策
1. **反馈循环方案**：采用 feedback_filter 吞没模式（而非 exclusive_gateway），因为框架不支持条件回边
2. **循环终止**：双重保护 — feedback_filter 吞没通过的消息 + LoopConfig::new(3) 限制最大迭代
3. **HITL 实现**：HumanTaskExecutor 两阶段执行（yield_output + request_halt，然后 resume 注入消息）
4. **output 节点**：p6_gateway 作为 output 节点，通过条件边投递的通过消息成为工作流输出
5. **AgentExecutor 包装**：每个 Agent 工厂返回 `Arc<dyn IAgent>`，用 `AgentExecutor::new(agent)` 包装为 `Arc<dyn IExecutor>`
6. **初始消息**：`ChatMessage::user(&initial_requirement)` 作为工作流初始输入

### 假设
1. `AgentExecutor` 在 `rust-agent-workflow` 中公开导出（需验证）
2. `HumanTaskExecutor` 在 `rust-agent-workflow` 中公开导出（已确认）
3. `WorkflowRuntime`, `ResumeCommand`, `WorkflowEvent` 公开导出（已确认）
4. `LoopConfig` 公开导出（已确认）
5. 测试中使用 mock/FunctionExecutor 避免真实 LLM API 调用
6. `add_fan_out_edge` 和 `add_fan_in_edge` 方法存在（已确认）

## 验证步骤

1. **编译验证**：`cargo build -p rust-agent-coding`
2. **单元测试**：`cargo test -p rust-agent-coding --lib`（state.rs 中的解析测试）
3. **集成测试**：`cargo test -p rust-agent-coding --test pipeline_build`
4. **集成测试**：`cargo test -p rust-agent-coding --test hitl_confirm`
5. **集成测试**：`cargo test -p rust-agent-coding --test parallel_coding`
6. **集成测试**：`cargo test -p rust-agent-coding --test feedback_loop`
7. **Clippy**：`cargo clippy -p rust-agent-coding -- -D warnings`
8. **格式化**：`cargo fmt -p rust-agent-coding -- --check`
9. **全量测试**：`cargo test -p rust-agent-coding`

## 实现顺序

1. 修改 `src/executors.rs` — 新增 `feedback_filter`
2. 修改 `src/conditions.rs` — 更新文档注释
3. 新建 `src/pipeline.rs` — 工作流图构建器
4. 新建 `src/lib.rs` — crate 根
5. 新建 `src/bin/coding.rs` — CLI 入口
6. 新建 `tests/pipeline_build.rs`
7. 新建 `tests/hitl_confirm.rs`
8. 新建 `tests/parallel_coding.rs`
9. 新建 `tests/feedback_loop.rs`
10. 运行验证步骤（编译、测试、clippy、fmt）
