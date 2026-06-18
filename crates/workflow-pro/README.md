# rust-agent-workflow-pro

Workflow Pro — 业务流程基础设施与高级编排层。

在 `rust-agent-workflow` 的图驱动编排引擎之上，提供可序列化流程定义、标准活动节点、Agent 团队管理、SAGA 事务补偿、审计追踪和 SLA 监控等业务基础设施。

## 架构分层

```
workflow-pro (业务层)
  ├── ProcessDefinition  →  生成  →  WorkflowGraph
  ├── ProcessInstance    →  委托  →  WorkflowEngine
  ├── ServiceTask / UserTask / ... (IExecutor 实现)
  ├── SagaOrchestrator (高层 SAGA)
  ├── AgentPool / Team / Router (Agent 管理)
  └── AuditTrail / Metrics / SLA (可观测性)
         │
         ▼ 依赖（仅通过 IExecutor / IWorkflowContext trait 交互）
workflow (引擎层)
  ├── WorkflowGraph / WorkflowEngine / IExecutor
  ├── SuperStep / Checkpoint / Retry / Timeout
  ├── TimerTrigger / CronTrigger (调度原语)
  ├── MessageCorrelation (消息关联)
  └── Enhanced Gateways / BoundaryEvent
```

## 快速开始

### ProcessDefinition — 流程定义 DSL

```rust
use rust_agent_workflow_pro::ProcessDefinition;

let yaml = r#"
id: order-process
name: Order Processing
version: "1.0"
nodes:
  - id: start
    kind: Start
  - id: validate
    kind: ServiceTask
    config:
      url: "https://api.example.com/validate"
  - id: end
    kind: End
edges:
  - id: e1
    source: start
    target: validate
  - id: e2
    source: validate
    target: end
"#;

let def = ProcessDefinition::from_yaml(yaml)?;
let graph = def.compile()?;  // -> WorkflowGraph
```

### ProcessInstance — 流程生命周期

```rust
use rust_agent_workflow_pro::{ProcessInstance, ProcessState};

let def = std::sync::Arc::new(ProcessDefinition::from_yaml(yaml)?);
let instance = ProcessInstance::new("inst-1", def);

assert_eq!(instance.state(), ProcessState::Created);
instance.start().unwrap();
instance.get_variable("key");
instance.set_variable("key", serde_json::json!("value"));
```

### SagaOrchestrator — 事务补偿

```rust
use rust_agent_workflow_pro::{SagaOrchestrator, SagaStep, SagaPolicy};

let saga = SagaOrchestrator::new()
    .step(SagaStep::new("create_order", create_exec)
        .with_compensation(cancel_exec))
    .step(SagaStep::new("process_payment", payment_exec)
        .with_compensation(refund_exec))
    .with_policy(SagaPolicy::BackwardRecovery);

saga.execute(msg, ctx).await?;
```

### BusinessVariables — 类型化变量

```rust
use rust_agent_workflow_pro::{BusinessVariables, VariableSchema};

let mut vars = BusinessVariables::new()
    .register(VariableSchema::string("name").required())
    .register(VariableSchema::number("amount"));

vars.set("name", serde_json::json!("order-123")).unwrap();
vars.get("name");
```

## 依赖

```toml
[dependencies]
rust-agent-workflow-pro = "0.1"
```

## License

MIT
