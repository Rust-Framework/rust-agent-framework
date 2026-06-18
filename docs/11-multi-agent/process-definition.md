# 11.11 流程定义与编译

`ProcessDefinition` 是 workflow-pro 提供的声明式流程定义 DSL，支持 YAML/JSON 序列化，并编译为 `WorkflowGraph` 由引擎驱动执行。本章全面介绍流程定义的建模语言、节点类型、边规则和编译机制。

## 流程定义模型

```rust
pub struct ProcessDefinition {
    pub id: String,
    pub name: String,
    pub version: String,
    pub nodes: Vec<NodeDef>,
    pub edges: Vec<EdgeDef>,
    pub variables: Vec<VariableDef>,
    pub timers: Vec<TimerDef>,
    pub events: Vec<BoundaryEventDef>,
}
```

核心方法：
- `from_yaml(s: &str) -> Result<ProcessDefinition>` — 从 YAML 字符串解析
- `compile() -> Result<WorkflowGraph>` — 编译为引擎可执行的不可变图
- `validate() -> Result<()>` — 校验流程定义的完整性

## 节点类型（16 种 NodeKind）

| 种类 | 说明 | 对应 Executor |
|------|------|--------------|
| `Start` | 流程入口节点 | 编译时创建 |
| `End` | 流程出口节点（标记为 output） | 编译时创建 |
| `ServiceTask` | 调用外部 HTTP 服务 | `ServiceTask` |
| `UserTask` | 人工审批/交互 | `UserTask` |
| `ScriptTask` | 内联脚本执行 | `ScriptTask` |
| `SendTask` | 发送消息到外部系统 | `SendTask` |
| `ReceiveTask` | 等待外部消息 | `ReceiveTask` |
| `BusinessRuleTask` | 业务规则决策 | `BusinessRuleTask` |
| `CallActivity` | 调用子流程 | `CallActivity` |
| `NoneTask` | 空占位节点 | `NoneTask` |
| `ParallelGateway` | 并行网关（FanOut） | 编译时创建 |
| `ExclusiveGateway` | 排他网关（条件分支） | 编译时创建 |
| `InclusiveGateway` | 包容网关（多条件并行分支） | 编译时创建 |
| `EventBasedGateway` | 事件驱动网关（等待多个事件中任一触发） | 编译时创建 |
| `TimerBoundary` | 定时器边界事件节点 | 编译时创建 |
| `ErrorBoundary` | 错误边界事件节点 | 编译时创建 |

## 节点定义

```rust
pub struct NodeDef {
    pub id: String,
    pub kind: NodeKind,      // 节点类型
    pub label: Option<String>,
    pub config: serde_json::Value,   // 节点特定配置（如 ServiceTask 的 url）
    pub retry: Option<RetryDef>,     // 重试策略
    pub timeout_ms: Option<u64>,     // 超时（ms）
}
```

## 边定义——支持条件路由

```rust
pub struct EdgeDef {
    pub id: String,
    pub source: String,     // 源节点 ID
    pub target: String,     // 目标节点 ID
    pub condition: Option<EdgeConditionDef>,
}

pub struct EdgeConditionDef {
    pub condition_type: String,    // "variable" 或 "expression"
    pub variable: Option<String>,  // 变量名
    pub operator: Option<String>,  // eq/neq/gt/gte/lt/lte/contains/starts_with
    pub expected: Option<serde_json::Value>,
}
```

## 变量与定时器

```rust
pub struct VariableDef {
    pub name: String,
    pub default_value: Option<serde_json::Value>,
    pub required: bool,
}

pub struct TimerDef {
    pub node_id: String,
    pub kind: String,       // "delay" 或 "cron"
    pub value: String,      // duration 字符串（"30s"）或 cron 表达式
}
```

## 从声明式到可执行

```rust
use rust_agent_workflow_pro::ProcessDefinition;

let yaml = r#"
id: order-flow
name: Order Processing
version: "1.0"
nodes:
  - id: start
    kind: Start
  - id: validate_gateway
    kind: ExclusiveGateway
  - id: valid_path
    kind: ServiceTask
    config:
      url: "https://api.example.com/process"
  - id: invalid_path
    kind: SendTask
  - id: end
    kind: End
edges:
  - id: e1
    source: start
    target: validate_gateway
  - id: e2
    source: validate_gateway
    target: valid_path
    condition:
      condition_type: variable
      variable: is_valid
      operator: eq
      expected: true
  - id: e3
    source: validate_gateway
    target: invalid_path
    condition:
      condition_type: variable
      variable: is_valid
      operator: eq
      expected: false
  - id: e4
    source: valid_path
    target: end
  - id: e5
    source: invalid_path
    target: end
"#;

// 1. 解析 YAML 为 ProcessDefinition
let def = ProcessDefinition::from_yaml(yaml)?;

// 2. 校验完整性
def.validate()?;

// 3. 编译为 WorkflowGraph（可提交给 WorkflowEngine 执行）
let graph = def.compile()?;
```

## 编译过程

`compile()` 的内部流程：

1. **节点注册**：为每个 `NodeDef` 创建对应的 `IExecutor`，通过 `WorkflowBuilder::add_node()` 注册
2. **网关转换**：识别 `ExclusiveGateway`/`ParallelGateway`/`InclusiveGateway` 并调用对应的 Builder DSL
3. **边路由**：按 `edges_by_source` 分组，将 `EdgeDef` 转换为 `WorkflowBuilder` 的边
4. **边界事件**：注册 `BoundaryEvent` 节点并在 `attached_to` 与 `event_node_id` 之间创建边
5. **输出标记**：所有 `End` 节点标记为 `with_output_from()`

编译产物是 `WorkflowGraph`——一个不可变的 DAG，可提交给 `WorkflowEngine` 驱动执行。

## 校验规则

`validate()` 检查：
- 至少有一个节点（不能空定义）
- 至少有一个 `Start` 节点
- 至少有一个 `End` 节点
- 所有边的 source/target 引用的节点 ID 必须存在
- 边界事件的 `attached_to` 必须引用已存在的节点
