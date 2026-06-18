# 11.12 流程实例与状态管理

`ProcessInstance` 是流程定义的运行时载体，管理业务流程从创建到完成的完整生命周期。每个 `ProcessInstance` 绑定一个 `ProcessDefinition`（蓝图），并通过 `WorkflowEngine` 驱动实际执行。

## 架构关系

```
ProcessDefinition（蓝图）
    ↓ new(inst_id, definition)
ProcessInstance（运行时）
    ↓ graph().clone()
WorkflowEngine（引擎）
    ↓ run(initial_message, session)
事件流 + 输出流
```

## 流程状态机

```
Created ──→ Running ──→ Completed
   │           │
   │           ├──→ Suspended ──→ Running（恢复）
   │           │
   │           ├──→ Terminated（强制终止）
   │           │
   │           └──→ Failed（执行错误）
   │
   ├──→ Terminated（创建后直接终止，不经运行）
   └──→ Failed（创建后直接标记失败）
```

```rust
pub enum ProcessState {
    Created,     // 已创建，尚未启动
    Running,     // 正在执行
    Suspended,   // 已挂起，等待恢复
    Completed,   // 正常完成
    Terminated,  // 强制终止
    Failed,      // 执行失败
}
```

## 核心 API

```rust
let def = Arc::new(ProcessDefinition::from_yaml(yaml)?);
let instance = ProcessInstance::new("order-001", def);

// 状态驱动
assert_eq!(instance.state(), ProcessState::Created);
instance.start()?;           // Created → Running
instance.suspend()?;         // Running → Suspended
instance.resume()?;          // Suspended → Running
instance.complete()?;        // Running → Completed

// 变量管理
instance.set_variable("amount", json!(150.0));
let amount = instance.get_variable("amount");

// 终止/失败
instance.terminate("用户取消")?;   // 强制终止
instance.fail("外部API超时")?;     // 标记失败
```

## 状态守卫

所有状态转换都有严格的守卫校验，防止非法操作：

| 操作 | 允许的状态 | 目标状态 |
|------|-----------|---------|
| `start()` | Created, Suspended | Running |
| `suspend()` | Running | Suspended |
| `resume()` | Suspended | Running |
| `complete()` | Running | Completed |
| `terminate()` | Created, Running, Suspended | Terminated |
| `fail()` | Created, Running, Suspended | Failed |

非法状态转换（如从未 Running 状态调用 `suspend()`）会返回 `AgentError::WorkflowError`。

## 流程快照

`ProcessSnapshot` 提供流程状态的完整快照，可用于持久化和审计：

```rust
pub struct ProcessSnapshot {
    pub process_id: String,
    pub definition_id: String,
    pub state: ProcessState,
    pub current_node_id: Option<String>,   // 当前活跃节点
    pub variables: HashMap<String, serde_json::Value>,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

let snapshot = instance.snapshot();
// → 可将 snapshot 持久化到数据库或会话存储
```

## 变量初始化

`ProcessInstance::new()` 自动初始化定义中声明的变量：
- 有 `default_value` 的变量 → 初始化为默认值
- `required = true` 且无默认值的变量 → 初始化为 `null`（标记为待填充）

## 与 WorkflowEngine 集成

```rust
// ProcessInstance.start() 内部完成：
// 1. 状态验证（必须为 Created）
// 2. 状态切换 → Running
// 3. 创建 WorkflowEngine 并 spawn 异步任务
// 4. 消费事件流和输出流
// 5. 正常完成 → Completed，错误 → Failed

instance.start(initial_message, session).await?;
```

引擎在独立 tokio 任务中运行，`ProcessInstance` 通过状态机同步引擎的执行结果。事件流（`WorkflowEvent`）和输出流（`WorkflowOutput`）可供外部消费者订阅。
