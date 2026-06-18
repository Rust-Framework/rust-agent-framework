# 11.16 增强网关、事件与定时调度

workflow 引擎层在原有 `ParallelGateway` / `ExclusiveGateway` / `InclusiveGateway` 基础上，新增了 `EventBasedGateway`、`ComplexGateway`、`BoundaryEvent`、`IntermediateEvent` 和 `TimerTrigger` / `CronTrigger` 调度原语。

## EventBasedGateway — 事件驱动网关

等待多个事件定义中任一触发，第一个匹配的事件决定路由路径。

```
Input → EventBasedGateway ─┬─ Timer(30s) → TimeoutHandler
                           ├─ Signal(CANCEL) → CancelHandler
                           └─ Message(payment_done) → PaymentHandler
```

```rust
use rust_agent_workflow::graph::EventBasedGatewayCondition;
use rust_agent_workflow::engine::BoundaryEventKind;

let timer_cond = EventBasedGatewayCondition::new(
    BoundaryEventKind::Timer(Duration::from_secs(30))
);

let cancel_cond = EventBasedGatewayCondition::new(
    BoundaryEventKind::Signal("CANCEL".into())
);

// 作为 IEdgeCondition 附加到排他网关的每条分支边上
builder = builder.exclusive_gateway(
    "gateway_id",
    vec![
        ("timeout_handler".into(), Arc::new(timer_cond)),
        ("cancel_handler".into(), Arc::new(cancel_cond)),
    ],
    Some("default_handler"),  // 兜底分支
);
```

## ComplexGateway — 复杂条件网关

支持多条件组合（AND/OR 逻辑）的路由决策。

```rust
use rust_agent_workflow::graph::{ComplexGatewayCondition, SubCondition, ComparisonOperator};

let condition = ComplexGatewayCondition::all_of(vec![
    SubCondition {
        variable: "amount".into(),
        operator: ComparisonOperator::GreaterThan,
        expected: json!(100),
    },
    SubCondition {
        variable: "approved".into(),
        operator: ComparisonOperator::Equals,
        expected: json!(true),
    },
]);
```

支持的运算符：`Equals`、`NotEquals`、`GreaterThan`、`GreaterThanOrEqual`、`LessThan`、`LessThanOrEqual`、`Contains`、`StartsWith`。

## BoundaryEvent — 边界事件

附加在节点上的中断或非中断事件。对应 BPMN 的 BoundaryEvent。

```rust
use rust_agent_workflow::engine::{BoundaryEvent, BoundaryEventKind};

// 定时器边界事件（30 秒超时 → 中断原节点）
let timeout_event = BoundaryEvent::timer(
    "order_node",
    Duration::from_secs(30),
    "timeout_handler"
);

// 信号边界事件（收到 CANCEL → 非中断并行分支）
let cancel_event = BoundaryEvent::signal("order_node", "CANCEL", "cancel_handler")
    .non_interrupting();
```

支持的边界事件类型：

| 类型 | 触发条件 |
|------|---------|
| `Timer(Duration)` | 超时触发 |
| `Error(String)` | 错误码匹配 |
| `Signal(String)` | 信号名称匹配 |
| `Message(String)` | 消息名称匹配 |
| `Escalation(String)` | 升级触发 |
| `Compensation` | 补偿触发 |

## IntermediateEvent — 中间事件

流程中的事件节点，支持 Catch（等待）和 Throw（触发）两种模式。

```rust
use rust_agent_workflow::engine::{IntermediateEvent, EventDefinition, IntermediateEventKind};

// Catch 模式：等待 timer 到期
let catch_event = IntermediateEvent::catch(
    "wait_5min",
    EventDefinition::Timer { duration: Some(Duration::from_secs(300)) },
);

// Throw 模式：触发信号
let throw_event = IntermediateEvent::throw(
    "emit_signal",
    EventDefinition::Signal { name: "ORDER_COMPLETE".into() },
);
```

## TimerTrigger — 延迟触发执行器

作为 `IExecutor` 插入工作流图。首次 `handle()` 注册 `schedule_timer()`，timer 到期后引擎自动 re-enqueue，再次 `handle()` 时透传消息。

```rust
use rust_agent_workflow::TimerTrigger;
use std::time::Duration;

let timer = TimerTrigger::new("delay_node", Duration::from_secs(10));

// 插入图中
builder = builder.add_node("delay_node", Arc::new(timer));
builder = builder.add_edge("source", "delay_node");
builder = builder.add_edge("delay_node", "downstream");
```

## CronTrigger — Cron 表达式调度

周期性触发执行器。支持的格式（6 段）：`秒 分 时 日 月 星期`（0=周日）。

```rust
use rust_agent_workflow::CronTrigger;

// 每 5 分钟触发一次
let cron = CronTrigger::new("scheduled_job", "0 */5 * * * *")
    .with_max_iterations(100)  // 最多触发 100 次
    .with_timer_name("daily_report");
```

Cron 字段支持：
- `*` — 任意值
- `*/N` — 每隔 N（如 `*/5` = 每 5 分钟）
- `1,15,30` — 特定值列表

每次触发后自动计算下次触发时间并重新 `schedule_timer()`。
