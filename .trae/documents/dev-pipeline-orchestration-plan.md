# 自动化软件开发智能体编排方案 — 实现计划

## 摘要

基于 RAF 框架的图驱动工作流引擎，新建 `rust-agent-coding` crate，以编程式 `WorkflowBuilder` 构建 6 阶段闭环开发编排管道。方案遵循"以终为始"哲学：需求分析（含人机确认）→ 测试驱动设计 → 架构设计 → 并行开发分解 → 回归测试 → 反馈循环。利用 `HumanTaskExecutor` 实现阶段 1 的人机确认，`FanOut/FanIn` 实现阶段 4 的并行编码，`LoopbackEdge + exclusive_gateway` 实现阶段 6 的反馈循环，`ContextFunctionExecutor` 实现跨阶段状态共享。

***

## 当前状态分析

### RAF 框架已提供的能力（直接复用）

| 能力       | 框架组件                                                                                      | 文件位置                                                  |
| -------- | ----------------------------------------------------------------------------------------- | ----------------------------------------------------- |
| 图驱动工作流   | `WorkflowBuilder`, `WorkflowGraph`                                                        | `crates/workflow/src/builder/workflow_builder.rs`     |
| Agent 节点 | `AgentExecutor`                                                                           | `crates/workflow/src/executor/agent_executor.rs`      |
| 纯函数节点    | `FunctionExecutor`                                                                        | `crates/workflow/src/executor/function_executor.rs`   |
| 上下文函数节点  | `ContextFunctionExecutor`                                                                 | `crates/workflow/src/executor/context_function.rs`    |
| 人工任务暂停   | `HumanTaskExecutor`                                                                       | `crates/workflow/src/executor/human_task.rs`          |
| 并行扇出/扇入  | `add_fan_out_edge`, `add_fan_in_edge`                                                     | `crates/workflow/src/builder/workflow_builder.rs`     |
| 条件网关     | `exclusive_gateway`, `IEdgeCondition`                                                     | `crates/workflow/src/graph/edge.rs`                   |
| 循环回边     | `add_loopback_edge`, `LoopConfig`                                                         | `crates/workflow/src/graph/node.rs`                   |
| 运行时暂停/恢复 | `WorkflowRuntime`, `ResumeCommand`                                                        | `crates/workflow/src/engine/runtime.rs`               |
| 工作流事件    | `WorkflowEvent` (Halted/Completed/Streaming)                                              | `crates/workflow/src/engine/event.rs`                 |
| 检查点恢复    | `CheckpointManager`, `WorkflowAgent::new_with_checkpoint`                                 | `crates/workflow/src/checkpoint/manager.rs`           |
| Agent 构建 | `AgentBuilder`                                                                            | `crates/framework/src/builder.rs`                     |
| LLM 客户端  | `DeepSeekChatClient`, `ChatClientOptions::deepseek`                                       | `crates/client/src/deepseek_client.rs`                |
| 文件工具     | `ReadFile`, `WriteFile`, `EditFile`, `RunCommand`, `ListFiles`, `SearchFile`, `FindFiles` | `crates/framework/src/tools/`                         |
| 工作区上下文   | `WorkspaceContextProvider`                                                                | `crates/framework/src/context_providers/workspace.rs` |
| 消息模型     | `ChatMessage`                                                                             | `crates/core/src/message.rs`                          |

### 关键设计约束

1. **AgentExecutor 不直接访问** **`IWorkflowContext`** — 需用 `ContextFunctionExecutor` 作为"上下文注入器"和"产物持久化器"包裹 Agent 节点
2. **消息沿边传递为** **`Arc<dyn Any>`** — AgentExecutor 输出为 `ChatMessage::assistant(text)`，下游可 downcast
3. **`HumanTaskExecutor`** **两阶段执行** — 首次 `yield_output + request_halt`，恢复后返回注入的审批结果
4. **`LoopbackEdge`** **需配合** **`LoopConfig`** — 在目标节点设置 `with_loop_on` 限制最大迭代次数

***

## 提议变更

### 1. 新建 crate: `crates/coding` (包名 `rust-agent-coding`)

#### 1.1 `Cargo.toml`

```toml
[package]
name = "rust-agent-coding"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
rust-agent-core = { workspace = true }
rust-agent-client = { workspace = true }
rust-agent-framework = { workspace = true }
rust-agent-workflow = { workspace = true }
tokio = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
anyhow = { workspace = true }
tracing = { workspace = true }
futures-util = { workspace = true }
async-trait = { workspace = true }

[dev-dependencies]
tempfile = { workspace = true }

[[bin]]
name = "coding"
path = "src/bin/coding.rs"
```

同时在根 `Cargo.toml` 的 `members` 数组末尾添加 `"crates/coding"`，在 `[workspace.dependencies]` 添加 `rust-agent-coding = { path = "crates/coding", version = "0.1.0" }`。

#### 1.2 文件清单

```
crates/coding/
├── Cargo.toml
├── src/
│   ├── lib.rs              # 库入口 + re-exports + 7 个 Agent 指令常量
│   ├── state.rs            # StateKey 常量 + 产物结构体
│   ├── conditions.rs       # ReviewPassedCondition (IEdgeCondition 实现)
│   ├── agents.rs           # 7 个专家 Agent 工厂函数
│   ├── executors.rs        # 3 类自定义执行器工厂函数
│   ├── pipeline.rs         # DevPipeline 主编排器 (图构建)
│   └── bin/
│       └── coding.rs       # 交互式二进制入口 (HITL)
└── tests/
    ├── pipeline_build_test.rs   # 图构建与验证
    ├── hitl_confirm_test.rs     # 人机确认暂停/恢复
    ├── parallel_coding_test.rs  # 并行编码 FanOut/FanIn
    └── feedback_loop_test.rs    # 反馈循环回边 + 网关
```

***

### 2. `src/state.rs` — 共享状态键与产物类型

定义跨阶段共享的状态键常量和产物结构体，供 `ContextFunctionExecutor` 读写。

```rust
/// 工作流共享状态键 — 所有阶段通过 IWorkflowContext::write_state/read_state 共享
pub mod state_keys {
    pub const REQUIREMENTS_DOC: &str = "requirements_doc";       // 阶段1: 需求分析文档
    pub const USER_CONFIRMATION: &str = "user_confirmation";     // 阶段1: 用户确认结果
    pub const TEST_CASES: &str = "test_cases";                   // 阶段2: 集成/冒烟测试用例
    pub const ARCHITECTURE_DOC: &str = "architecture_doc";       // 阶段3: 架构设计文档
    pub const TASK_PLAN: &str = "task_plan";                     // 阶段4a: 任务分解计划
    pub const CODE_CHANGES_ALPHA: &str = "code_changes_alpha";   // 阶段4b: coder-alpha 变更
    pub const CODE_CHANGES_BETA: &str = "code_changes_beta";     // 阶段4b: coder-beta 变更
    pub const REGRESSION_RESULTS: &str = "regression_results";   // 阶段5: 回归测试结果
    pub const REVIEW_FEEDBACK: &str = "review_feedback";         // 阶段6: 审查反馈
    pub const ITERATION_COUNT: &str = "iteration_count";         // 反馈循环计数
}

/// 审查结论 — 用于 exclusive_gateway 条件判断
#[derive(Deserialize)]
pub struct ReviewVerdict {
    pub passed: bool,                    // 全部预期是否达成
    pub discrepancies: Vec<String>,      // 差异点列表
    pub root_cause: String,              // 根因分析（需求/设计/实现）
    pub fix_suggestions: Vec<String>,    // 修复建议
}
```

***

### 3. `src/conditions.rs` — 反馈循环网关条件

实现 `IEdgeCondition`，解析 reviewer 输出判断是否通过。

```rust
use rust_agent_workflow::IEdgeCondition;
use rust_agent_workflow::MessageEnvelope;

/// 审查通过条件 — 解析 ReviewVerdict.passed
/// 用于 exclusive_gateway: passed=true → output, passed=false → loopback
pub struct ReviewPassedCondition;

#[async_trait]
impl IEdgeCondition for ReviewPassedCondition {
    async fn evaluate(&self, envelope: &MessageEnvelope) -> Result<bool> {
        // 尝试从 content (Arc<dyn Any>) downcast 到 ChatMessage
        // 提取 assistant 文本，解析 JSON 中的 "passed" 字段
        // 返回 true 表示通过（走 output 分支）
    }
}
```

***

### 4. `src/agents.rs` — 7 个专家 Agent 工厂

每个 Agent 用 `AgentBuilder` 构建，配置专属指令、工具集、工具轮次上限。所有 Agent 共享同一个 `Arc<dyn IChatClient>`。

#### 4.1 `create_requirements_analyst(client, workspace_root) -> Arc<dyn IAgent>`

**阶段 1: 需求分析智能体**

* **指令核心**: 全面分解需求，重点分析表现形态

  * 服务接口：API 定义、请求/响应结构、场景测试说明

  * 应用界面：界面效果、用户交互场景、业务场景解决

  * 扩展性：性能要求、风险、挑战

  * 输出结构化需求文档（Markdown）

* **工具**: `WriteFile`（写需求文档）, `ReadFile`, `ListFiles`（了解现有项目）

* **max\_tool\_rounds**: 10

#### 4.2 `create_test_designer(client, workspace_root) -> Arc<dyn IAgent>`

**阶段 2: 测试驱动设计智能体**

* **指令核心**: 根据需求文档编写集成测试和冒烟测试用例

  * 对照最终结果编写完整集成测试

  * 编写冒烟测试验证核心链路

  * 固化最终交付结果形态

  * 测试用例从使用侧视角编写（体验和结果）

* **工具**: `WriteFile`, `ReadFile`, `ListFiles`, `SearchFile`

* **max\_tool\_rounds**: 12

#### 4.3 `create_architect(client, workspace_root) -> Arc<dyn IAgent>`

**阶段 3: 架构设计智能体**

* **指令核心**: 围绕需求+测试结果设计最佳软件架构

  * 明确项目结构、文件分布、代码职责

  * 区分架构固定/扩展/业务实现部分

  * 明确集成、联调、对接方式

  * 技术为业务服务，架构为需求服务

* **工具**: `WriteFile`, `ReadFile`, `ListFiles`, `FindFiles`, `SearchFile`

* **max\_tool\_rounds**: 10

#### 4.4 `create_task_planner(client, workspace_root) -> Arc<dyn IAgent>`

**阶段 4a: 开发任务分解智能体**

* **指令核心**: 遵循高内聚低耦合原则拆分可并行编码内容

  * 任务规划严格绑定前三步目标

  * 拆分为 coder-alpha / coder-beta 两个并行工作包

  * 明确每个工作包的文件/模块边界（避免冲突）

  * 每个功能点需先写单元测试

* **工具**: `WriteFile`, `ReadFile`, `ListFiles`

* **max\_tool\_rounds**: 8

#### 4.5 `create_coder(client, workspace_root, agent_id: &str) -> Arc<dyn IAgent>`

**阶段 4b: 并行开发者（模板函数，生成 alpha/beta）**

* **指令核心**: 实现分配的工作包

  * 每个功能点开发前先编写单元测试

  * 单元测试目标围绕最终集成产出

  * 功能点完成后必须通过单元测试

  * 遵循项目既有风格，最小必要改动

  * 禁止降级产出，不允许牺牲目标质量

* **工具**: `ReadFile`, `WriteFile`, `EditFile`, `RunCommand`, `SearchFile`, `ListFiles`

* **max\_tool\_rounds**: 20

#### 4.6 `create_regression_tester(client, workspace_root) -> Arc<dyn IAgent>`

**阶段 5: 回归测试智能体**

* **指令核心**: 全链路回归测试

  * 执行集成测试、冒烟测试

  * 验证全链路结果与设计预期一致性

  * 对照阶段 2 的测试用例逐项验证

  * 输出 PASS/FAIL 报告，失败项给出详细日志

* **工具**: `RunCommand`, `ReadFile`, `ListFiles`, `SearchFile`

* **max\_tool\_rounds**: 15

#### 4.7 `create_reviewer(client, workspace_root) -> Arc<dyn IAgent>`

**阶段 6: 反馈审查智能体**

* **指令核心**: 审查实际结果与预期差异

  * 对照需求文档、测试用例、架构设计

  * 每个差异点回归初始需求审查

  * 输出结构化 JSON: `{ "passed": bool, "discrepancies": [...], "root_cause": "...", "fix_suggestions": [...] }`

  * 根因分类：需求问题/设计问题/实现问题

* **工具**: `ReadFile`, `RunCommand`, `ListFiles`, `SearchFile`

* **max\_tool\_rounds**: 12

***

### 5. `src/executors.rs` — 3 类自定义执行器工厂

#### 5.1 `artifact_persist(node_id, state_key, file_path) -> Arc<ContextFunctionExecutor>`

**产物持久化器** — 接收上游 Agent 输出，写入工作流状态 + 文件系统

```rust
pub fn artifact_persist(
    node_id: &str,
    state_key: &'static str,
    file_path: Option<PathBuf>,
) -> Arc<dyn IExecutor> {
    Arc::new(ContextFunctionExecutor::new(node_id, move |msg, ctx, _progress| {
        let state_key = state_key;
        let file_path = file_path.clone();
        async move {
            // 1. Downcast msg 到 ChatMessage，提取 assistant 文本
            let text = extract_assistant_text(&msg)?;
            // 2. 写入工作流共享状态
            ctx.write_state(state_key, serde_json::Value::String(text.clone())).await?;
            // 3. 可选：写入文件系统（通过 ctx.session 或直接 std::fs）
            if let Some(path) = &file_path {
                std::fs::write(path, &text)?;
            }
            // 4. 透传消息给下游
            Ok(HandlerResult::Messages(vec![msg]))
        }
    }))
}
```

#### 5.2 `context_inject(node_id, state_keys: Vec<&'static str>, prompt_template: &str) -> Arc<ContextFunctionExecutor>`

**上下文注入器** — 从工作流状态读取多个产物，构建富上下文 prompt 消息传给下游 Agent

```rust
pub fn context_inject(
    node_id: &str,
    state_keys: Vec<&'static str>,
    prompt_template: String,
) -> Arc<dyn IExecutor> {
    Arc::new(ContextFunctionExecutor::new(node_id, move |_msg, ctx, _progress| {
        let state_keys = state_keys.clone();
        let template = prompt_template.clone();
        async move {
            // 1. 读取所有指定状态键
            let mut artifacts = String::new();
            for key in &state_keys {
                if let Some(val) = ctx.read_state(key).await? {
                    artifacts.push_str(&format!("## {} \n{}\n\n", key, val));
                }
            }
            // 2. 填充模板，构建 ChatMessage::user
            let prompt = template.replace("{artifacts}", &artifacts);
            let message = ChatMessage::user(&prompt);
            // 3. yield_output 供可观测性
            ctx.yield_output(Arc::new(message.clone())).await?;
            // 4. 传递给下游 AgentExecutor
            Ok(HandlerResult::Messages(vec![Arc::new(message)]))
        }
    }))
}
```

#### 5.3 `code_merger(node_id) -> Arc<FunctionExecutor>`

**代码合并器** — FanIn 聚合器，合并两个并行 coder 的输出

```rust
pub fn code_merger(node_id: &str) -> Arc<dyn IExecutor> {
    Arc::new(FunctionExecutor::new(node_id, |msgs: Vec<ChatMessage>| {
        let merged: String = msgs.iter()
            .map(|m| m.content_text())
            .collect::<Vec<_>>()
            .join("\n\n---\n\n");
        vec![ChatMessage::assistant(&merged)]
    }))
}
```

***

### 6. `src/pipeline.rs` — 主编排器 (核心)

#### 6.1 `DevPipelineConfig` 配置结构

```rust
pub struct DevPipelineConfig {
    pub workspace_root: PathBuf,
    pub artifacts_dir: PathBuf,       // 产物输出目录 (如 .coding/artifacts/)
    pub max_iterations: usize,        // 反馈循环最大迭代 (默认 10)
    pub model: String,                // LLM 模型 ID
    pub api_key: String,              // API Key
}
```

#### 6.2 `DevPipeline::build(config) -> WorkflowGraph`

构建 6 阶段闭环工作流图。**节点命名约定**: `{phase}_{role}_{action}`

```
图拓扑:

[entry]                                          ← FunctionExecutor (pass-through, 初始消息)
  │
  ├─ Phase 1: 需求分析
  │   [p1_analyst]                                ← AgentExecutor (requirements_analyst)
  │   [p1_persist]                                ← ContextFunctionExecutor (artifact_persist → requirements_doc)
  │   [p1_confirm]                                ← HumanTaskExecutor (用户确认)
  │
  ├─ Phase 2: 测试驱动设计
  │   [p2_inject]                                 ← ContextFunctionExecutor (读 requirements_doc, 构建 prompt)
  │   [p2_designer]                               ← AgentExecutor (test_designer)
  │   [p2_persist]                                ← ContextFunctionExecutor (artifact_persist → test_cases)
  │
  ├─ Phase 3: 架构设计
  │   [p3_inject]                                 ← ContextFunctionExecutor (读 requirements_doc + test_cases)
  │   [p3_architect]                              ← AgentExecutor (architect)
  │   [p3_persist]                                ← ContextFunctionExecutor (artifact_persist → architecture_doc)
  │
  ├─ Phase 4a: 任务分解
  │   [p4a_inject]                                ← ContextFunctionExecutor (读 req + tests + arch)
  │   [p4a_planner]                               ← AgentExecutor (task_planner)
  │   [p4a_persist]                               ← ContextFunctionExecutor (artifact_persist → task_plan)
  │
  ├─ Phase 4b: 并行编码 (FanOut → FanIn)
  │   [p4b_fanout]                                ← FunctionExecutor (source, pass-through)
  │     ├── [p4b_alpha_inject] → [p4b_alpha_coder] → [p4b_alpha_persist]   ← coder-alpha 链
  │     └── [p4b_beta_inject]  → [p4b_beta_coder]  → [p4b_beta_persist]    ← coder-beta 链
  │   [p4b_merger]                                ← FunctionExecutor (code_merger, FanIn 聚合)
  │
  ├─ Phase 5: 回归测试
  │   [p5_inject]                                 ← ContextFunctionExecutor (读所有产物)
  │   [p5_tester]                                 ← AgentExecutor (regression_tester)
  │   [p5_persist]                                ← ContextFunctionExecutor (artifact_persist → regression_results)
  │
  ├─ Phase 6: 反馈循环
  │   [p6_inject]                                 ← ContextFunctionExecutor (读所有产物 + regression_results)
  │   [p6_reviewer]                               ← AgentExecutor (reviewer)
  │   [p6_persist]                                ← ContextFunctionExecutor (artifact_persist → review_feedback)
  │   [p6_gateway]                                ← FunctionExecutor (pass-through, 网关源节点)
  │     │
  │     ├── exclusive_gateway:
  │     │   (ReviewPassedCondition=true)  → [output]    ← 完成
  │     │   (default)                     → loopback → [p4a_inject]  ← 反馈回跳到任务分解
  │
  └─ [output]                                     ← FunctionExecutor (终端输出节点)
```

#### 6.3 关键构建代码（伪代码）

```rust
pub fn build(config: &DevPipelineConfig) -> Result<WorkflowGraph> {
    let client = Arc::new(DeepSeekChatClient::new(
        ChatClientOptions::deepseek(&config.model, &config.api_key)
    )?);

    // 构建 7 个专家 Agent
    let analyst = create_requirements_analyst(client.clone(), &config.workspace_root);
    let test_designer = create_test_designer(client.clone(), &config.workspace_root);
    let architect = create_architect(client.clone(), &config.workspace_root);
    let planner = create_task_planner(client.clone(), &config.workspace_root);
    let coder_alpha = create_coder(client.clone(), &config.workspace_root, "coder-alpha");
    let coder_beta = create_coder(client.clone(), &config.workspace_root, "coder-beta");
    let tester = create_regression_tester(client.clone(), &config.workspace_root);
    let reviewer = create_reviewer(client.clone(), &config.workspace_root);

    let mut b = WorkflowBuilder::new();

    // ── 入口 ──
    b = b.add_node("entry", pass_through("entry"));
    b = b.set_start("entry");

    // ── Phase 1: 需求分析 + 人机确认 ──
    b = b.add_agent_node("p1_analyst", analyst);
    b = b.add_edge("entry", "p1_analyst");
    b = b.add_node("p1_persist", artifact_persist("p1_persist", state_keys::REQUIREMENTS_DOC, Some(config.artifacts_dir.join("requirements.md"))));
    b = b.add_edge("p1_analyst", "p1_persist");
    b = b.add_node("p1_confirm", Arc::new(HumanTaskExecutor::new(
        "p1_confirm",
        Arc::new(|_ctx| serde_json::json!({
            "type": "requirements_review",
            "message": "请审阅需求分析结果。回复 {\"approved\": true} 确认，或 {\"approved\": false, \"corrections\": \"...\"} 提出修正。"
        })),
    )));
    b = b.add_edge("p1_persist", "p1_confirm");

    // ── Phase 2: 测试驱动设计 ──
    b = b.add_node("p2_inject", context_inject("p2_inject", vec![state_keys::REQUIREMENTS_DOC],
        "根据以下需求文档，编写集成测试和冒烟测试用例。从使用侧视角验证最终交付结果形态。\n\n{artifacts}".into()));
    b = b.add_edge("p1_confirm", "p2_inject");
    b = b.add_agent_node("p2_designer", test_designer);
    b = b.add_edge("p2_inject", "p2_designer");
    b = b.add_node("p2_persist", artifact_persist("p2_persist", state_keys::TEST_CASES, Some(config.artifacts_dir.join("test_cases.md"))));
    b = b.add_edge("p2_designer", "p2_persist");

    // ── Phase 3: 架构设计 ──
    b = b.add_node("p3_inject", context_inject("p3_inject",
        vec![state_keys::REQUIREMENTS_DOC, state_keys::TEST_CASES],
        "根据以下需求和测试用例，设计软件架构。明确项目结构、文件分布、代码职责、集成方式。\n\n{artifacts}".into()));
    b = b.add_edge("p2_persist", "p3_inject");
    b = b.add_agent_node("p3_architect", architect);
    b = b.add_edge("p3_inject", "p3_architect");
    b = b.add_node("p3_persist", artifact_persist("p3_persist", state_keys::ARCHITECTURE_DOC, Some(config.artifacts_dir.join("architecture.md"))));
    b = b.add_edge("p3_architect", "p3_persist");

    // ── Phase 4a: 任务分解 ──
    b = b.add_node("p4a_inject", context_inject("p4a_inject",
        vec![state_keys::REQUIREMENTS_DOC, state_keys::TEST_CASES, state_keys::ARCHITECTURE_DOC],
        "根据以下需求、测试和架构，分解开发任务为可并行工作包（coder-alpha / coder-beta）。每个功能点需先写单元测试。\n\n{artifacts}".into()));
    b = b.add_edge("p3_persist", "p4a_inject");
    // 设置循环入口（反馈回跳目标）
    b = b.with_loop_on("p4a_inject", LoopConfig::new(config.max_iterations));
    b = b.add_agent_node("p4a_planner", planner);
    b = b.add_edge("p4a_inject", "p4a_planner");
    b = b.add_node("p4a_persist", artifact_persist("p4a_persist", state_keys::TASK_PLAN, Some(config.artifacts_dir.join("task_plan.md"))));
    b = b.add_edge("p4a_planner", "p4a_persist");

    // ── Phase 4b: 并行编码 (FanOut/FanIn) ──
    // alpha 链
    b = b.add_node("p4b_alpha_inject", context_inject("p4b_alpha_inject",
        vec![state_keys::TASK_PLAN, state_keys::ARCHITECTURE_DOC],
        "你是 coder-alpha。实现分配给你的工作包。每个功能点先写单元测试再实现。\n\n{artifacts}".into()));
    b = b.add_agent_node("p4b_alpha_coder", coder_alpha);
    b = b.add_node("p4b_alpha_persist", artifact_persist("p4b_alpha_persist", state_keys::CODE_CHANGES_ALPHA, None));
    b = b.add_edge("p4a_persist", "p4b_alpha_inject");
    b = b.add_edge("p4b_alpha_inject", "p4b_alpha_coder");
    b = b.add_edge("p4b_alpha_coder", "p4b_alpha_persist");

    // beta 链
    b = b.add_node("p4b_beta_inject", context_inject("p4b_beta_inject",
        vec![state_keys::TASK_PLAN, state_keys::ARCHITECTURE_DOC],
        "你是 coder-beta。实现分配给你的工作包。每个功能点先写单元测试再实现。\n\n{artifacts}".into()));
    b = b.add_agent_node("p4b_beta_coder", coder_beta);
    b = b.add_node("p4b_beta_persist", artifact_persist("p4b_beta_persist", state_keys::CODE_CHANGES_BETA, None));
    b = b.add_edge("p4a_persist", "p4b_beta_inject");
    b = b.add_edge("p4b_beta_inject", "p4b_beta_coder");
    b = b.add_edge("p4b_beta_coder", "p4b_beta_persist");

    // FanIn 合并
    b = b.add_node("p4b_merger", code_merger("p4b_merger"));
    b = b.add_fan_in_edge(vec!["p4b_alpha_persist", "p4b_beta_persist"], "p4b_merger");

    // ── Phase 5: 回归测试 ──
    b = b.add_node("p5_inject", context_inject("p5_inject",
        vec![state_keys::REQUIREMENTS_DOC, state_keys::TEST_CASES, state_keys::ARCHITECTURE_DOC],
        "执行全链路回归测试，验证结果与设计预期一致性。对照测试用例逐项验证。\n\n{artifacts}".into()));
    b = b.add_edge("p4b_merger", "p5_inject");
    b = b.add_agent_node("p5_tester", tester);
    b = b.add_edge("p5_inject", "p5_tester");
    b = b.add_node("p5_persist", artifact_persist("p5_persist", state_keys::REGRESSION_RESULTS, Some(config.artifacts_dir.join("regression_report.md"))));
    b = b.add_edge("p5_tester", "p5_persist");

    // ── Phase 6: 反馈循环 ──
    b = b.add_node("p6_inject", context_inject("p6_inject",
        vec![state_keys::REQUIREMENTS_DOC, state_keys::TEST_CASES, state_keys::REGRESSION_RESULTS],
        "审查实际结果与预期差异。输出 JSON: {{\"passed\": bool, \"discrepancies\": [...], \"root_cause\": \"...\", \"fix_suggestions\": [...]}}\n\n{artifacts}".into()));
    b = b.add_edge("p5_persist", "p6_inject");
    b = b.add_agent_node("p6_reviewer", reviewer);
    b = b.add_edge("p6_inject", "p6_reviewer");
    b = b.add_node("p6_persist", artifact_persist("p6_persist", state_keys::REVIEW_FEEDBACK, None));
    b = b.add_edge("p6_reviewer", "p6_persist");

    // 网关: 通过 → output, 失败 → loopback 到 p4a_inject
    b = b.add_node("p6_gateway", pass_through("p6_gateway"));
    b = b.add_edge("p6_persist", "p6_gateway");
    b = b.add_node("output", pass_through("output"));
    b = b.exclusive_gateway("p6_gateway",
        vec![("output", Arc::new(ReviewPassedCondition))],
        Some("p4a_inject"));  // default → loopback
    b = b.add_loopback_edge("p6_gateway", "p4a_inject");  // 显式标记回边
    b = b.with_output_from("output");

    b.build()
}
```

**注意**: `exclusive_gateway` 的 default\_branch 指向 `p4a_inject`，配合 `add_loopback_edge` 显式标记回边，`with_loop_on("p4a_inject", LoopConfig::new(max_iterations))` 限制最大迭代。当 `ReviewPassedCondition` 为 true 时走 `output`，否则走 default 回跳。

***

### 7. `src/bin/coding.rs` — 交互式二进制入口

```rust
/// 用法: coding --requirement "实现一个 REST API..." --workspace . --model deepseek-v4-flash
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. 解析 CLI 参数 (clap 或手动解析)
    let config = parse_args()?;

    // 2. 构建工作流图
    let graph = DevPipeline::build(&config)?;

    // 3. 启动 WorkflowRuntime
    let initial: Arc<dyn Any + Send + Sync> = Arc::new(ChatMessage::user(&config.requirement));
    let runtime = WorkflowRuntime::start(graph, initial, None).await?;

    // 4. 消费事件流 — HITL 交互循环
    let mut events = runtime.events().await.unwrap();
    let mut outputs = runtime.outputs().await.unwrap();

    loop {
        tokio::select! {
            Some(event) = events.next() => {
                match event {
                    WorkflowEvent::NodeStreaming { node_id, chunk } => {
                        // 打印 Agent 流式输出 (打字机效果)
                        print_streaming_chunk(&node_id, &chunk);
                    }
                    WorkflowEvent::WorkflowHalted { .. } => {
                        // HITL: 读取 yield_output 的产物，呈现给用户
                        let artifact = outputs.next().await;
                        present_requirements_to_user(artifact)?;
                        // 读取用户输入
                        let confirmation = read_user_confirmation()?;
                        // 恢复工作流
                        runtime.resume(ResumeCommand::InjectMessage {
                            target_node_id: "p1_confirm".into(),
                            message: Arc::new(confirmation),
                        })?;
                    }
                    WorkflowEvent::WorkflowCompleted { .. } => {
                        println!("✓ 开发流程完成");
                        break;
                    }
                    WorkflowEvent::WorkflowError { error, .. } => {
                        eprintln!("✗ 错误: {}", error);
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    runtime.wait().await?;
    Ok(())
}
```

***

### 8. `src/lib.rs` — 库入口

```rust
pub mod state;
pub mod conditions;
pub mod agents;
pub mod executors;
pub mod pipeline;

pub use pipeline::{DevPipeline, DevPipelineConfig};
pub use state::state_keys;
pub use conditions::ReviewPassedCondition;
```

***

### 9. 测试计划

#### 9.1 `tests/pipeline_build_test.rs`

验证图构建与校验：

* `build()` 返回 Ok

* 图包含所有预期节点（entry, p1\_analyst, ..., output）

* 入口节点为 entry

* 输出节点为 output

* FanOut/FanIn 边正确连接

* loopback 边存在且标记正确

#### 9.2 `tests/hitl_confirm_test.rs`

验证人机确认暂停/恢复：

* 用 stub ChatClient（返回固定文本）避免真实 LLM 调用

* 启动 WorkflowRuntime

* 断言收到 `WorkflowHalted` 事件

* 调用 `runtime.resume(InjectMessage { target: "p1_confirm", message: json!({"approved": true}) })`

* 断言工作流继续执行到 p2\_inject

#### 9.3 `tests/parallel_coding_test.rs`

验证并行编码：

* 用 stub ChatClient

* 断言 p4b\_alpha\_coder 和 p4b\_beta\_coder 都被调用

* 断言 p4b\_merger 收到两条消息并合并

#### 9.4 `tests/feedback_loop_test.rs`

验证反馈循环：

* 用 stub ChatClient，reviewer 返回 `{"passed": false}`

* 断言工作流回跳到 p4a\_inject

* 第二轮 reviewer 返回 `{"passed": true}`

* 断言工作流到达 output

* 验证 LoopConfig 限制最大迭代

***

## 假设与决策

### 决策

1. **交付形态**: 新建独立 crate `rust-agent-coding`（路径 `crates/coding`），符合 RAF 工作区模块化约定，可被其他项目复用
2. **HITL 机制**: `HumanTaskExecutor` 工作流暂停，阶段 1 后暂停等待用户确认，通过 `ResumeCommand::InjectMessage` 恢复
3. **实现风格**: 编程式 `WorkflowBuilder`，最大灵活性表达 HITL/并行/回环/状态共享
4. **状态共享**: `ContextFunctionExecutor` + `IWorkflowContext::write_state/read_state`，每个 Agent 前后各一个上下文节点
5. **反馈回跳目标**: 回跳到 `p4a_inject`（任务分解阶段），携带 review\_feedback 重新规划。若根因为需求/设计问题，reviewer 可在 fix\_suggestions 中说明，planner 会据此调整
6. **并行编码**: 2 个 coder（alpha/beta），通过 FanOut/FanIn 并行。可扩展为更多 coder
7. **指令内联**: Agent 指令以 Rust 字符串常量内联在 `agents.rs` 中，避免额外文件
8. **Stub 客户端测试**: 测试中使用实现 `IChatClient` 的 stub，返回固定文本，避免依赖真实 LLM

### 假设

1. 工作区根 `Cargo.toml` 可添加新成员
2. `ContextFunctionExecutor` 的闭包可捕获 `&'static str` 状态键（无需克隆）
3. `HumanTaskExecutor` 的 `task_builder` 闭包可访问 `IWorkflowContext` 读取上游产物构建确认表单
4. `exclusive_gateway` 的 default\_branch 配合 `add_loopback_edge` 可实现条件回跳（框架支持）
5. `FunctionExecutor` 可接受 `Vec<ChatMessage>` 类型参数（FanIn 聚合场景）

***

## 验证步骤

1. **编译验证**: `cargo build -p rust-agent-coding` 成功
2. **单元测试**: `cargo test -p rust-agent-coding` 全部通过

   * 图构建测试验证拓扑正确

   * HITL 测试验证暂停/恢复

   * 并行测试验证 FanOut/FanIn

   * 反馈循环测试验证回跳与终止
3. **集成验证**: `cargo run -p rust-agent-coding -- --requirement "实现一个简单的 echo REST API" --workspace . --model deepseek-v4-flash`

   * 阶段 1 后暂停，用户确认后继续

   * 各阶段产物写入 `.coding/artifacts/` 目录

   * 反馈循环在预期通过后终止
4. **Lint**: `cargo clippy -p rust-agent-coding -- -D warnings` 无警告
5. **格式**: `cargo fmt -p rust-agent-coding -- --check` 通过

