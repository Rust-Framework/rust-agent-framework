# 12.8 消息关联、审计与 SLA

workflow 引擎层和 workflow-pro 业务层共同提供了消息关联匹配（`MessageCorrelation`）、审计追踪（`AuditTrail`）、消息代理抽象（`IMessageBroker`）和 SLA 监控（`SlaTracker`）等可观测性和集成能力。

## MessageCorrelation — 消息关联匹配

`MessageCorrelation` 实现 `IEdgeCondition`，通过关联键（`CorrelationKey`）将入站消息与等待中的流程实例匹配。

### 关联键定义

```rust
use rust_agent_workflow::engine::{CorrelationKey, MessageCorrelation};

// 通过业务主键关联
let key = CorrelationKey::by_business_key("order-12345");
// 通过流程实例 ID 关联
let key = CorrelationKey::by_process_id("proc-abc");
// 自定义多键组合（AND 逻辑）
let key = CorrelationKey::by_business_key("order-12345")
    .with_custom_key("type", "payment")
    .with_custom_key("channel", "web");

// 创建关联器（可选超时）
let correlation = MessageCorrelation::new(key)
    .with_timeout(Duration::from_hours(1));

// 作为 IEdgeCondition 附加到边上
builder = builder.add_edge_with_condition(
    "source",
    "message_handler",
    Arc::new(correlation),
);
```

### 匹配规则

从 `MessageEnvelope.metadata` 中查找关联字段（AND 逻辑全部匹配）：

| 关联键 | envelope metadata 字段 |
|--------|----------------------|
| `business_key` | `"business_key"` |
| `process_id` | `"process_id"` |
| `custom_keys` | 对应 key 的 metadata 字段 |

```rust
// ReceiveTask 使用示例
let msg_env = MessageEnvelope::new("source", msg, TypeTag::new("test"))
    .with_metadata("business_key", json!("order-12345"))
    .with_metadata("type", json!("payment"));

let key = CorrelationKey::by_business_key("order-12345")
    .with_custom_key("type", "payment");
assert!(key.matches(&msg_env));  // → true
```

### 超时支持

`MessageCorrelation` 内置超时检测：

```rust
let correlation = MessageCorrelation::new(key)
    .with_timeout(Duration::from_secs(30));

correlation.start();              // 开始计时
// ... 等待消息 ...
correlation.is_timed_out();       // 检查是否超时
correlation.reset();              // 重置计时器
```

## IMessageBroker — 消息代理抽象

外部消息中间件（Kafka、RabbitMQ、Redis 等）的集成接口。

```rust
use rust_agent_workflow_pro::IMessageBroker;

#[async_trait]
pub trait IMessageBroker: Send + Sync {
    // 发布消息到指定主题
    async fn publish(
        &self, topic: &str, payload: &serde_json::Value,
        headers: Option<HashMap<String, String>>,
    ) -> Result<(), String>;

    // 订阅指定主题
    async fn subscribe(&self, topic: &str)
        -> Result<Box<dyn MessageReceiver>, String>;

    // RPC 请求模式（发后等回）
    async fn request(
        &self, topic: &str, payload: &serde_json::Value,
        timeout_ms: u64,
    ) -> Result<serde_json::Value, String>;

    // 健康检查
    async fn health_check(&self) -> bool;
}
```

### InMemoryMessageBroker

框架内置内存实现，用于测试和简单场景：

```rust
use rust_agent_workflow_pro::message::broker::InMemoryMessageBroker;

let broker = InMemoryMessageBroker::new();

// 发布
broker.publish("orders", &json!({"id": 123}), None).await?;

// 订阅
let mut receiver = broker.subscribe("orders").await?;
let msg: Option<ReceivedMessage> = receiver.recv(1000).await?;
```

## AuditTrail — 审计追踪

`AuditTrail` 消费 `WorkflowEvent` 流，将关键事件转换为 `AuditEntry` 记录持久化。

```rust
use rust_agent_workflow_pro::AuditTrail;

let trail = AuditTrail::new(1000);  // 保留最近 1000 条

// 记录 Info 事件
trail.info("proc-1", "node_exec", "Node started");

// 记录 Error 事件
trail.error("proc-1", "node_exec", "Node failed");

// 查询指定流程的所有审计条目
let entries: Vec<AuditEntry> = trail.entries_for("proc-1");
// → 按 process_id 过滤

// 查看全部
let all: Vec<AuditEntry> = trail.all();
```

### AuditEntry 结构

```rust
pub struct AuditEntry {
    pub id: String,                    // UUID
    pub process_id: String,            // 流程实例 ID
    pub node_id: Option<String>,       // 节点 ID
    pub level: AuditLevel,             // Info / Warning / Error / Critical
    pub category: String,              // "node_exec" / "gateway_route" / "compensation"
    pub message: String,               // 人类可读描述
    pub data: Option<serde_json::Value>,  // 附加结构化数据
    pub timestamp: DateTime<Utc>,
    pub duration_ms: Option<u64>,      // 执行耗时
}
```

## SlaTracker — SLA 追踪

`SlaTracker` 基于流程定义的 SLA 截止时间追踪节点和流程级别的时效性。

```rust
use rust_agent_workflow_pro::{SlaTracker, SlaDeadline, SlaStatus};

let tracker = SlaTracker::new(vec![
    SlaDeadline::new("overall", Duration::from_secs(60)),
    SlaDeadline::new("validation", Duration::from_secs(10))
        .for_node("validate_order"),
]);

// 启动所有 SLA 计时
tracker.start_all();

// 定期检查
let statuses: Vec<(String, SlaStatus)> = tracker.check_all();
for (name, status) in &statuses {
    println!("{} → {:?}", name, status);
}

// 单项完成
tracker.complete("validation");

// 查看剩余时间
let remaining = tracker.remaining("overall");  // → Some(Duration)
```

### 状态演进

```
Pending → OnTrack → AtRisk (>80% 已用) → Breached (超时)
                                              ↓
                                            Met (在 SLA 内完成)
```

### 违约回调

```rust
tracker.on_breach(|deadline_name: &str| {
    // 发送告警、触发升级等
    tracing::warn!("SLA breached: {}", deadline_name);
});
```

## ProcessMetricsCollector — 指标采集

收集流程实例的执行指标，包括节点数、完成数、失败数、重试数和耗时。

```rust
use rust_agent_workflow_pro::ProcessMetricsCollector;

let collector = ProcessMetricsCollector::new();

// 注册流程
collector.register("proc-1");

// 记录节点事件
collector.node_started("proc-1", "node-a");
collector.node_completed("proc-1", "node-a", Duration::from_millis(120));
collector.node_failed("proc-1", "node-b");
collector.node_retried("proc-1");

// 完工
collector.process_completed("proc-1");

// 查询
let metrics = collector.get("proc-1").unwrap();
println!("成功率: {:.1}%", metrics.success_rate() * 100.0);
println!("总耗时: {}ms", metrics.duration_ms());
```
