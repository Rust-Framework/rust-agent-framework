# 11.8 自定义编排（WorkflowBuilder）

当内置编排模式无法满足需求时，`WorkflowBuilder` 提供了构建任意有向图拓扑的能力。你可以定义自定义节点、边、条件路由、网关和循环，创建完全定制的工作流。

## WorkflowBuilder 架构

```mermaid
graph LR
    subgraph "构建阶段"
        WB[WorkflowBuilder]
        WB --> N[add_node / add_agent_node]
        WB --> E[add_edge / add_fan_out_edge / add_fan_in_edge]
        WB --> C[add_edge_with_condition]
        WB --> L[add_loopback_edge]
        WB --> G[parallel_gateway / exclusive_gateway / inclusive_gateway]
    end

    subgraph "构建产物"
        GPH[WorkflowGraph]
    end

    subgraph "执行阶段"
        GPH --> WE[WorkflowEngine]
        WE --> EVT[WorkflowEvent 流]
        WE --> OUT[WorkflowOutput 流]
    end

    WB -->|build()| GPH
```

## 核心概念

### Node（节点）

每个节点绑定一个执行器，支持重试、超时和循环配置：

```rust
// 基础节点
builder = builder.add_node("node_id", my_executor);

// Agent 节点（快捷方式）
builder = builder.add_agent_node("agent_node", my_agent);

// 带重试策略
builder = builder.add_node("flaky", executor).with_retry(RetryOptions {
    max_retries: 3,
    backoff: RetryBackoff::Exponential { base: Duration::from_secs(1), max: Duration::from_secs(30) },
    retry_on: RetryCondition::AllErrors,
    on_exhausted: ExhaustedAction::Skip,
});

// 带超时
builder = builder.add_node("slow", executor).with_node_timeout(Duration::from_secs(60));

// 带循环配置
builder = builder.add_node("loop_node", executor).with_loop(LoopConfig::new(10));
```

### Edge（边）

支持四种边类型，构成完整的消息路由拓扑：

| 边 | API | 行为 |
|----|-----|------|
| 直接边 | `.add_edge(src, dst)` | 1:1 消息路由 |
| 条件边 | `.add_edge_with_condition(src, dst, condition)` | 仅条件为真时路由 |
| 扇出边 | `.add_fan_out_edge(src, vec![...])` | 消息零拷贝广播到所有目标 |
| 扇入边 | `.add_fan_in_edge(vec![...], dst)` | 所有源到达后才触发目标 |
| 循环边 | `.add_loopback_edge(src, dst)` | 显式标记循环回边，图校验允许 |

### Gateway（网关）DSL

`WorkflowBuilder` 提供声明式网关语法糖，不改变底层边模型：

```rust
// 并行网关 —— 创建 FanOut 边
builder.parallel_gateway("entry", vec!["branch_a", "branch_b", "branch_c"]);

// 排他网关 —— 精一条分支激活
let cond_a = Arc::new(VariableEdgeCondition::new("status", ComparisonOp::Eq, json!("success")));
let cond_b = Arc::new(VariableEdgeCondition::new("status", ComparisonOp::Eq, json!("error")));
builder.exclusive_gateway(
    "checkpoint",
    vec![("success_path", cond_a), ("error_path", cond_b)],
    Some("fallback_path"),  // 默认分支
);

// 包容网关 —— 所有满足条件的分支并行执行
builder.inclusive_gateway(
    "dispatch",
    vec![("path_a", cond_a), ("path_b", cond_b)],
);
```

### Condition（条件）

内置三种条件类型，均实现 `IEdgeCondition` trait：

| 条件类型 | 用途 |
|---------|------|
| `VariableCondition` | 基于单个流程变量比较（Eq/Neq/Gt/Gte/Lt/Lte/Contains/StartsWith） |
| `ExpressionCondition` | 多条件组合（AllOf/AnyOf） |
| `VariableEdgeCondition` | 基于变量名和值的条件，从 `state_map` 读取 |

```rust
use rust_agent_workflow::{VariableCondition, ExpressionCondition, ComparisonOp};

let cond_eq = VariableCondition::new("status", ComparisonOp::Eq, json!("success"));
let cond_gt = VariableCondition::new("score", ComparisonOp::Gt, json!(0.8));
let cond_contains = VariableCondition::new("output", ComparisonOp::Contains, json!("approved"));

let combined = ExpressionCondition::all_of(vec![cond_eq, cond_gt]);
```

### LoopConfig（循环）

支持显式标记循环回边，引擎管理迭代和终止：

```rust
pub struct LoopConfig {
    pub max_iterations: usize,        // 最大迭代次数（0 表示无限制）
    pub loop_variable: Option<String>, // 循环变量名，自动递增
}

// 审批驳回重审循环
let approval_loop = LoopConfig::new(5).with_variable("approval_round");

// 推理循环（由 ITerminationCondition 控制）
let reasoning_loop = LoopConfig::unlimited();
```

循环迭代状态可序列化到 checkpoint，支持恢复后继续迭代。

## 执行器类型

### AgentExecutor — IAgent 执行器

```rust
use rust_agent_workflow::AgentExecutor;

let executor = AgentExecutor::new("agent_node", my_agent);
builder.add_node("node", Arc::new(executor));
```

当节点被激活时，`AgentExecutor` 调用 `agent.run()` 并将流式输出转换为工作流消息。

### FunctionExecutor — 函数执行器

```rust
use rust_agent_workflow::FunctionExecutor;

let executor = FunctionExecutor::new("transformer", |msg: String| -> Vec<String> {
    vec![format!("处理结果: {}", msg.to_uppercase())]
});
```

泛型参数 I→O，函数签名自动推导输入/输出类型。

### HumanTaskExecutor — 人工任务执行器

```rust
use rust_agent_workflow::HumanTaskExecutor;

let executor = HumanTaskExecutor::new(
    "approval",
    Arc::new(|ctx| serde_json::json!({"form": "请确认是否继续？"})),
);
```

首次调用时构造审批表单、暂停工作流。外部通过 `WorkflowRuntime::resume(InjectMessage {...})` 注入审批结果后恢复。

### SubFlowExecutor — 子流程执行器

```rust
use rust_agent_workflow::SubFlowExecutor;

let executor = SubFlowExecutor::new("subflow",
    Arc::new(|ctx| {
        WorkflowBuilder::new()
            .add_node(/* ... */)
            .build()
            .unwrap()
    }),
);
```

运行时构造子图并作为独立 WorkflowEngine 执行。

### CompensableExecutor — 补偿执行器

```rust
use rust_agent_workflow::CompensableExecutor;

let executor = CompensableExecutor::new(
    inner_executor,
    |ctx: &dyn IWorkflowContext| async move {
        // 补偿逻辑：回滚已执行的操作
        println!("回滚操作");
        Ok(())
    },
);
```

当后续节点失败时，引擎沿执行链反向调用 `compensate()` 实现 Saga 回滚。

## 完整示例：数据分析工作流

```rust
use rust_agent_workflow::{
    WorkflowBuilder, WorkflowEngine,
    AgentExecutor, FunctionExecutor,
    VariableCondition, ExpressionCondition, ComparisonOp, LoopConfig,
    ExhaustedAction, RetryBackoff, RetryOptions,
};
use std::sync::Arc;
use std::time::Duration;

async fn build_analysis_workflow(
    analyzer: Arc<dyn IAgent>,
    reporter: Arc<dyn IAgent>,
) -> anyhow::Result<WorkflowGraph> {
    WorkflowBuilder::new()
        // 1. 数据预处理（FunctionExecutor）
        .add_node("preprocess", Arc::new(FunctionExecutor::new(
            "preprocess",
            |msg: String| vec![format!("{{ \"data\": \"{}\", \"timestamp\": \"{}\" }}", msg, chrono::Utc::now())],
        )))
        .set_start("preprocess")

        // 2. AI 分析（AgentExecutor）
        .add_agent_node("analyze", analyzer)

        // 3. 质量检查（FunctionExecutor）
        .add_node("quality_check", Arc::new(FunctionExecutor::new(
            "quality_check",
            |msg: String| {
                let score = if msg.len() > 200 { 0.8 } else { 0.3 };
                vec![serde_json::json!({"score": score, "passed": score > 0.5}).to_string()]
            },
        )))

        // 4. 报告生成（AgentExecutor），带重试
        .add_agent_node("report", reporter)
        .with_retry(RetryOptions {
            max_retries: 2,
            backoff: RetryBackoff::Fixed(Duration::from_secs(2)),
            retry_on: RetryCondition::AllErrors,
            on_exhausted: ExhaustedAction::Skip,
        })

        // 5. 回退节点
        .add_node("fallback", Arc::new(FunctionExecutor::new(
            "fallback",
            |_: String| vec!["分析质量不足以生成报告。请重新提交查询。".to_string()],
        )))

        // ── 边 ──
        .add_edge("preprocess", "analyze")
        .add_edge("analyze", "quality_check")

        // 排他网关：质量检查 → 报告 或 回退
        .exclusive_gateway(
            "quality_check",
            vec![("report", Arc::new(VariableEdgeCondition::new("score", ComparisonOp::Gt, serde_json::json!(0.5))))],
            Some("fallback"),
        )

        .with_output_from("report")
        .with_output_from("fallback")
        .build()
}
```

## 图验证

`WorkflowBuilder::build()` 自动执行 `WorkflowGraph::validate()`：

1. **入口节点存在**：`start_node_id` 必须在 nodes 中
2. **边引用完整性**：所有边的源和目标节点必须已注册
3. **输出节点存在**：`with_output_from()` 标记的节点必须已注册
4. **BFS 可达性**：警告不可达节点
5. **DFS 环检测**：拒绝非循环回边的环，允许 `is_loopback = true` 的边

## 适用场景

| 场景 | 涉及特性 |
|------|---------|
| 复杂业务流程 | 条件路由、排他网关、多级审核 |
| 人机协同 | HumanTaskExecutor + WorkflowRuntime |
| Saga 事务 | CompensableExecutor + 补偿 |
| 迭代推理 | LoopConfig + 循环边 |
| 嵌套编排 | SubFlowExecutor 子图 |
