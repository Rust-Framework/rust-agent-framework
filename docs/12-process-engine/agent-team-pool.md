# 12.7 Agent 团队与池化管理

workflow-pro 提供 Agent 团队组织（`AgentTeam`）、连接池管理（`AgentPool`）和动态路由（`DynamicRouter`）能力，支持多 Agent 系统中的角色划分、负载均衡和智能调度。

## AgentTeam — 角色与能力注册

`AgentTeam` 将 Agent 按角色和能力分组，支持按能力查找 Agent。

```rust
use rust_agent_workflow_pro::{AgentTeam, AgentRole, AgentCapability};

let mut team = AgentTeam::new("code-review-team");

// 定义角色
let code_role = AgentRole {
    name: "code_expert".into(),
    description: "代码审查专家".into(),
    capabilities: vec![
        AgentCapability { name: "rust".into(), tags: vec!["async".into(), "tokio".into()] },
        AgentCapability { name: "python".into(), tags: vec!["data".into()] },
    ],
};

let review_role = AgentRole {
    name: "reviewer".into(),
    description: "代码审查员".into(),
    capabilities: vec![
        AgentCapability { name: "review".into(), tags: vec![] },
    ],
};

// 注册 Agent
team.register_agent(rust_expert, code_role);
team.register_agent(review_agent, review_role);

// 按能力查找
let experts = team.find_agent_by_capability("rust");
// → 返回所有 capability.name == "rust" 或 capability.tags 含 "rust" 的 Agent
```

### AgentCapability

```rust
pub struct AgentCapability {
    pub name: String,       // 能力名称（如 "rust", "code_review"）
    pub tags: Vec<String>,  // 附加标签（如 ["async", "backend"]）
}
```

### 查找策略

- 精确匹配 `capability.name`
- 模糊匹配 `capability.tags`
- 无匹配时返回所有已注册 Agent

## AgentPool — 连接池与健康检查

`AgentPool` 管理 Agent 实例池，支持连接复用、心跳检测和健康状态跟踪。

```rust
use rust_agent_workflow_pro::{AgentPool, AgentPoolConfig, AgentHealth};

let config = AgentPoolConfig {
    min_size: 1,
    max_size: 10,
    idle_timeout: Duration::from_secs(300),
    heartbeat_interval: Duration::from_secs(30),
};

let pool = AgentPool::new(config);

// 添加 Agent
pool.add_agent(agent_a);
pool.add_agent(agent_b);

// 获取健康 Agent
if let Some(agent) = pool.acquire() {
    // 使用 agent...
    // 自动记录 last_used、request_count
}

// 定时心跳（检查空闲超时）
pool.heartbeat();

// 查看健康状态
let status: Vec<(String, AgentHealth)> = pool.health_status();
```

### 健康状态

| 状态 | 含义 |
|------|------|
| `Healthy` | 上次使用未超过 idle_timeout |
| `Degraded` | 空闲超时，可能需回收 |
| `Unhealthy` | 不可用 |

### 获取策略

`pool.acquire()` 优先返回 `Healthy` 状态的 Agent。如果无健康 Agent 可用，返回 `None`（合理回退至上层处理）。

### PooledAgent 指标

每个池化 Agent 自动维护：
- `last_used`：最后使用时间
- `health`：当前健康状态
- `request_count`：已处理的请求数

## DynamicRouter — 能力路由

`DynamicRouter` 实现 `IEdgeCondition`，可附加在图边上按消息内容动态路由到匹配的 Agent。

```rust
use rust_agent_workflow_pro::DynamicRouter;

let router = DynamicRouter::new(
    vec![code_agent, writing_agent, search_agent],
    "code"  // capability_keyword
);

// 作为 IEdgeCondition 附加到边上
// 当消息内容（String）包含 "code" 时该边被激活
builder = builder.add_edge_with_condition(
    "source",
    "code_expert",
    Arc::new(router),
);
```

路由匹配基于 `AgentMetadata` 的以下字段：
- `description`：Agent 功能描述
- `agent_type`：Agent 类型名（如 "ChatClientAgent"）
- `capability_tags`：能力标签列表
