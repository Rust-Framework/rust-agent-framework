# 11.16 增强网关、事件与定时调度

workflow 引擎层在原有 ParallelGateway / ExclusiveGateway / InclusiveGateway 基础上，新增 EventBasedGateway、ComplexGateway、BoundaryEvent、IntermediateEvent 和 TimerTrigger / CronTrigger 调度原语。

## EventBasedGateway -- 事件驱动网关

等待多个事件定义中任一触发，第一个匹配的事件决定路由路径。

```
Input -> EventBasedGateway -+- Timer(30s) -> TimeoutHandler
                            +- Signal(CANCEL) -> CancelHandler
                            +- Message(done) -> DoneHandler
```

```rust
use rust_agent_workflow::graph::EventBasedGatewayCondition;
use rust_agent_workflow::engine::BoundaryEventKind;

let timer_cond = EventBasedGatewayCondition::new(
    BoundaryEventKind::Timer(Duration::from_secs(30))
);

builder = builder.exclusive_gateway("gw", vec![
    ("timeout_handler".into(), Arc::new(timer_cond)),
], Some("default_handler"));
```

## ComplexGateway -- 复杂条件网关

支持 AND/OR 组合条件的路由决策。

```rust
use rust_agent_workflow::graph::{ComplexGatewayCondition, SubCondition, ComparisonOperator};

let condition = ComplexGatewayCondition::all_of(vec![
    SubCondition { variable: "amount".into(), operator: ComparisonOperator::GreaterThan, expected: json!(100) },
    SubCondition { variable: "approved".into(), operator: ComparisonOperator::Equals, expected: json!(true) },
]);
```

运算符：Equals、NotEquals、GreaterThan、GreaterThanOrEqual、LessThan、LessThanOrEqual、Contains、StartsWith。

## BoundaryEvent -- 边界事件

附加在节点上的中断或非中断事件。

```rust
use rust_agent_workflow::engine::{BoundaryEvent, BoundaryEventKind};

// 定时器边界事件（30 秒超时 -> 中断）
let timeout_event = BoundaryEvent::timer(
    "order_node", Duration::from_secs(30), "timeout_handler"
);

// 信号边界事件（非中断并行分支）
let cancel_event = BoundaryEvent::signal("order_node", "CANCEL", "cancel_handler")
    .non_interrupting();
```

边界事件类型：Timer、Error、Signal、Message、Escalation、Compensation。

## IntermediateEvent -- 中间事件

Catch（等待）或 Throw（触发）事件。

```rust
use rust_agent_workflow::engine::{IntermediateEvent, EventDefinition};

let catch = IntermediateEvent::catch("wait",
    EventDefinition::Timer { duration: Some(Duration::from_secs(300)) }
);
let throw = IntermediateEvent::throw("emit",
    EventDefinition::Signal { name: "ORDER_DONE".into() }
);
```

## TimerTrigger -- 延迟触发

首次 handle() 注册 schedule_timer()，到期后引擎自动 re-enqueue。

```rust
use rust_agent_workflow::TimerTrigger;

let timer = TimerTrigger::new("delay", Duration::from_secs(10));
builder = builder.add_node("delay", Arc::new(timer));
```

## CronTrigger -- Cron 表达式调度

格式（6 段）：秒 分 时 日 月 星期。支持 *、*/N、1,15,30。

```rust
use rust_agent_workflow::CronTrigger;

let cron = CronTrigger::new("job", "0 */5 * * * *")
    .with_max_iterations(100);  // 每 5 分钟触发，最多 100 次
```
