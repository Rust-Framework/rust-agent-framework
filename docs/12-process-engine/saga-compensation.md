# 12.6 SAGA 事务与补偿链

`SagaOrchestrator` 提供声明式分布式事务编排，基于引擎已有的 `ICompensable` 和逆序 `compensate()` 机制，支持向前恢复和向后恢复两种策略。

## 设计理念

在分布式系统中，强一致性事务代价高昂。SAGA 模式将长事务分解为一系列本地事务步骤，每个步骤都有一个补偿操作。失败时逆序执行补偿，将系统恢复到一致状态。

```
Step 1: CreateOrder  --→  Step 2: ReserveInventory  --→  Step 3: ProcessPayment
      │                            │                            │
      ▼                            ▼                            ▼
Comp 1: CancelOrder         Comp 2: ReleaseInventory      Comp 3: RefundPayment

失败在 Step 3 → 触发 BackwardRecovery：
  → Comp 3 (RefundPayment) → Comp 2 (ReleaseInventory) → Comp 1 (CancelOrder)
```

## Saga 恢复策略

| 策略 | 行为 | 适用场景 |
|------|------|---------|
| `BackwardRecovery` | 失败时逆序执行已完成步骤的 `compensate()` | 需要严格数据一致性（如电商下单） |
| `ForwardRecovery` | 忽略失败，继续执行后续步骤 | 允许部分失败的场景（如多路并行试错） |

## 快速开始

```rust
use rust_agent_workflow_pro::{SagaOrchestrator, SagaStep, SagaPolicy};

// 定义正向操作
let create_order: Arc<dyn IExecutor> = /* ... */;
let reserve_stock: Arc<dyn IExecutor> = /* ... */;
let process_payment: Arc<dyn IExecutor> = /* ... */;

// 定义补偿操作
let cancel_order: Arc<dyn IExecutor> = /* ... */;
let release_stock: Arc<dyn IExecutor> = /* ... */;
let refund_payment: Arc<dyn IExecutor> = /* ... */;

let saga = SagaOrchestrator::new()
    .step(SagaStep::new("create_order", create_order)
        .with_compensation(cancel_order))
    .step(SagaStep::new("reserve_stock", reserve_stock)
        .with_compensation(release_stock))
    .step(SagaStep::new("process_payment", process_payment)
        .with_compensation(refund_payment))
    .with_policy(SagaPolicy::BackwardRecovery);

// 执行
let results = saga.execute(initial_message, ctx).await?;
```

## SagaStep 定义

```rust
pub struct SagaStep {
    pub name: String,
    pub action: Arc<dyn IExecutor>,         // 正向操作
    pub compensation: Option<Arc<dyn IExecutor>>,  // 补偿操作
}
```

补偿操作是可选的——如果某个步骤没有副作用（如只读查询），可以不设置补偿。

## 执行流程

`SagaOrchestrator::execute()` 的工作流程：

1. **顺序执行**：从第一步开始，逐步执行 `action.handle()`
2. **消息传递**：上一步的输出作为下一步的输入（`initial_message` 用于第一步）
3. **结果收集**：通过 `HandlerResult::Messages` / `Output` 收集每步输出
4. **失败处理**：
   - `BackwardRecovery`：记录已完成步骤的索引，失败时逆序调用 `compensate()`
   - `ForwardRecovery`：记录警告日志，继续下一步
5. **补偿执行**：每个补偿步骤通过独立的 `progress_tx` 通道执行，补偿失败仅记录日志不阻塞

## 与引擎 ICompensable 的关系

`SagaOrchestrator` 是高层的声明式封装，底层依赖引擎的两个补偿机制：

- `ICompensable` trait：标记某个 `IExecutor` 是否支持补偿
- `compensate()` 方法：由引擎在节点失败时逆序调用

`SagaOrchestrator` 将这些底层机制组织为声明式的步骤链，提供 `BackwardRecovery` / `ForwardRecovery` 策略选择。
