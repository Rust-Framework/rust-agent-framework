# 12.3 标准活动节点

workflow-pro 提供 8 种 BPMN 风格的标准活动节点，每种都实现了 `IExecutor` trait，可直接插入 `WorkflowGraph` 或通过 `ProcessDefinition` 声明式引用。

## 节点一览

| 节点 | 对应 BPMN | 核心行为 |
|------|----------|---------|
| `ServiceTask` | Service Task | 调用外部 HTTP API |
| `UserTask` | User Task | 暂停等待人工审批 |
| `ScriptTask` | Script Task | 执行内联脚本 |
| `SendTask` | Send Task | 通过 `IMessageBroker` 发送消息 |
| `ReceiveTask` | Receive Task | 基于 `CorrelationKey` 等待外部消息 |
| `BusinessRuleTask` | Business Rule Task | 评估业务规则并分支路由 |
| `CallActivity` | Call Activity | 加载并执行子流程 |
| `NoneTask` | None Task | 空占位节点 |

## ServiceTask

调用外部 HTTP 服务。支持 GET/POST 方法、自定义 headers 和超时控制。

```rust
let task = ServiceTask::new("http_call", "https://api.example.com/endpoint", "POST")
    .with_header("Authorization", "Bearer token123")
    .with_timeout(Duration::from_secs(30));
```

执行时通过 `tracing::info!` 记录请求详情，将原始消息透传下游。

## UserTask

增强型人工任务——首次调用时产出表单并暂停流程，恢复时传递审批结果。

```rust
let form_schema = json!({
    "title": "审批申请",
    "fields": [
        {"name": "approved", "type": "boolean", "label": "是否同意"},
        {"name": "comment", "type": "string", "label": "审批意见"}
    ]
});

let task = UserTask::new("approval", form_schema)
    .with_assignee("manager@example.com")
    .with_deadline(Duration::from_hours(24));
```

执行流程：
1. **首次调用**：构建表单（嵌入 assignee / deadline），通过 `yield_output()` 产出给外部消费者，然后 `request_halt_with_payload()` 暂停
2. **恢复调用**：接收 `serde_json::Value` 或 `String` 类型的审批结果，传递给下游

## ScriptTask

执行内联脚本（默认 Rhai 语言）。

```rust
let task = ScriptTask::new("calc", r#"total = amount * rate; return total;"#)
    .with_language("rhai");
```

## SendTask

通过 `IMessageBroker` 向外部系统发送消息。

```rust
let task = SendTask::new("notify", "order.created")
    .with_correlation_key("order-123");
```

执行时发出 `NodeProgress::Custom` 进度事件。

## ReceiveTask

基于 `CorrelationKey` 等待外部消息。首次调用时注册关联键并暂停，收到匹配消息后恢复并传递。

```rust
let key = CorrelationKey::by_business_key("order-456");
let task = ReceiveTask::new("wait_payment", key)
    .with_timeout(Duration::from_hours(1));
```

## BusinessRuleTask

基于流程变量评估业务规则，按匹配结果分支路由。

```rust
pub struct RuleDef {
    pub name: String,
    pub expression: String,     // 如 "amount > 1000 AND approved == true"
    pub result_branch: String,  // 匹配后路由到的分支名
}

let task = BusinessRuleTask::new("risk_check", vec![
    RuleDef { name: "high_risk".into(), expression: "amount > 10000".into(), result_branch: "manual_review".into() },
    RuleDef { name: "auto_approve".into(), expression: "approved == true".into(), result_branch: "process".into() },
]);
```

支持的表达式格式：`key == "value"`, `key != "value"`, 以及裸变量真值判断。

## CallActivity

调用子流程（通过 `IProcessRepository` 加载定义 → 编译 → 嵌入执行）。

```rust
let task = CallActivity::new("subprocess", "sub-order-validation");
```

注：当前实现为占位，通过 `tracing::info!` 记录子流程调用意图并透传消息。

## NoneTask

空占位节点——直接透传消息，常用于网关连接锚点。

```rust
let task = NoneTask::new("placeholder");
```