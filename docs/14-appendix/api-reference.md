# 14.1 API 速查表

本文档提供 RAF 框架所有核心公共 API 的快速参考，按 Crate 组织。

## rust-agent-core

基础抽象层，不依赖其他 RAF Crate。

### IAgent trait

```rust
pub trait IAgent: Send + Sync {
    fn id(&self) -> &AgentId;
    fn metadata(&self) -> &AgentMetadata;
    async fn run(&self, messages: Vec<ChatMessage>, session: Option<Arc<dyn ISession>>, options: Option<AgentRunOptions>) -> Result<BoxStream<'static, Result<AgentResponseResult>>>;
    fn get_subagent(&self, id: &AgentId) -> Option<Arc<dyn IAgent>>;
    async fn reset(&self) -> Result<()>;
    fn create_session(&self) -> Arc<dyn ISession>;
    fn deserialize_session(&self, data: &str) -> Result<Arc<dyn ISession>>;
    fn chat_client(&self) -> Option<&Arc<dyn IChatClient>>;
}
```

### ITool trait

```rust
pub trait ITool: AsAny + Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> serde_json::Value;
    async fn execute(&self, arguments: serde_json::Value) -> Result<ToolResult>;
    fn requires_approval(&self) -> bool;
}
```

### IContextProvider trait

```rust
pub trait IContextProvider: Send + Sync {
    fn name(&self) -> &str;
    async fn on_invoking(&self, agent: &dyn IAgent, session: &dyn ISession, messages: &[ChatMessage], options: &AgentRunOptions) -> Result<ContextInjection>;
    async fn on_invoked(&self, agent: &dyn IAgent, session: &dyn ISession, request_messages: &[ChatMessage], response: Option<&AgentResponse>, error: Option<&AgentError>) -> Result<()>;
}
```

### IChatClient trait

```rust
pub trait IChatClient: Send + Sync {
    fn model_id(&self) -> &str;
    fn model_metadata(&self) -> Option<&ModelMetadata>;
    async fn run(&self, messages: &[ChatMessage], options: ChatClientRunOptions) -> Result<BoxStream<'static, Result<AgentResponseUpdate>>>;
    fn inner_client(&self) -> Option<&Arc<dyn IChatClient>>;
}
```

### ISession trait

```rust
pub trait ISession: Send + Sync {
    fn session_id(&self) -> &str;
    async fn messages(&self) -> Vec<ChatMessage>;
    async fn add_message(&self, message: ChatMessage) -> Result<()>;
    async fn add_messages(&self, messages: Vec<ChatMessage>) -> Result<()>;
    async fn clear(&self) -> Result<()>;
    fn get_provider_state(&self, key: &str) -> Result<Option<serde_json::Value>>;
    fn set_provider_state(&self, key: &str, value: serde_json::Value) -> Result<()>;
}
```

### 关键类型

| 类型 | 字段 | 说明 |
|------|------|------|
| `ChatMessage` | `role`, `content`, `name`, `tool_calls`, `tool_call_id` | 对话消息 |
| `ToolResult` | `ok`, `data`, `error` | 工具执行结果 |
| `ContextInjection` | `instructions`, `messages`, `tools`, `replace_messages` | 上下文注入 |
| `AgentRunOptions` | `max_rounds`, `temperature`, `max_tokens`, `instructions_override`, `stream` | 运行选项 |
| `AgentResponseResult` | `contents: Vec<Content>` | Agent 响应块 |
| `AgentMetadata` | `agent_type`, `key`, `description`, `tool_names`, `model_id`, `capability_tags` | Agent 元数据 |
| `AgentSession` | impl ISession | 默认会话实现 |

## rust-agent-framework

Agent 运行时框架。

### AgentBuilder

```rust
pub struct AgentBuilder<C: IChatClient> {
    pub fn new(id: &str) -> Self;
    pub fn chat_client(self, client: C) -> Self;
    pub fn instructions(self, instructions: &str) -> Self;
    pub fn with_description(self, desc: &str) -> Self;
    pub fn with_tool(self, tool: impl ITool + 'static) -> Self;
    pub fn with_context_provider(self, provider: impl IContextProvider + 'static) -> Self;
    pub fn max_tool_rounds(self, max: usize) -> Self;
    pub fn build(self) -> Result<Arc<dyn IAgent>>;
}
```

### 内置工具

| 工具 | 说明 |
|------|------|
| `ReadFile` | 读取文件内容 |
| `WriteFile` | 写入文件 |
| `EditFile` | 编辑文件（基于文本替换） |
| `ListFiles` | 列出目录内容 |
| `SearchFile` | 搜索文件 |
| `FindFiles` | 按 glob 模式查找文件 |
| `InspectFile` | 检查文件元信息 |
| `MakeDirectory` | 创建目录 |
| `RemovePath` | 删除文件或目录 |
| `MoveFile` | 移动/重命名文件 |
| `RunCommand` | 执行 Shell 命令 |

### 上下文提供器

| 提供器 | 说明 |
|--------|------|
| `AgentSkillsProvider` | 技能注入提供器 |
| `SkillMemoryContextProvider` | 持久记忆提供器 |
| `InMemoryHistoryProvider` | 会话历史提供器 |
| `WorkspaceContextProvider` | 工作区上下文提供器 |

## rust-agent-workflow

工作流编排引擎。核心链路：`Builder.build() → Workflow.as_agent() → IAgent`。

### 编排模式 Builder

| 类型 | 核心方法 |
|------|---------|
| `SequentialWorkflowBuilder` | `new()`, `add_agent()`, `build()` |
| `SequentialWorkflow` | `from_agents()`, `run()`, `run_agent()`, `as_agent()` |
| `ConcurrentWorkflowBuilder` | `new()`, `add_agent()`, `with_agents()`, `build()` |
| `ConcurrentWorkflow` | `from_agents()`, `run()`, `run_agent()`, `as_agent()` |
| `HandoffWorkflowBuilder` | `new()`, `triage()`, `add_agent()`, `build()` |
| `HandoffWorkflow` | `run()`, `run_agent()`, `find_agent()`, `as_agent()` |
| `GroupChatWorkflowBuilder` | `new()`, `add_participant()`, `coordinator()`, `max_rounds()`, `build()` |
| `GroupChatWorkflow` | `run()`, `run_agent()`, `as_agent()` |
| `MagenticWorkflowBuilder` | `new()`, `orchestrator()`, `add_sub_agent()`, `add_tool()`, `max_iterations()`, `build()` |
| `MagenticWorkflow` | `run()`, `run_agent()`, `as_agent()`, `sub_agent_map()` |
| `VoteWorkflowBuilder` | `new()`, `add_voter()`, `aggregator()`, `voting_rounds()`, `build()` |
| `VoteWorkflow` | `run()`, `run_agent()`, `as_agent()` |
| `WorkflowBuilder` | `new()`, `add_node()`, `add_agent_node()`, `add_edge()`, `add_edge_with_condition()`, `add_fan_out_edge()`, `add_fan_in_edge()`, `add_loopback_edge()`, `parallel_gateway()`, `exclusive_gateway()`, `inclusive_gateway()`, `set_start()`, `with_output_from()`, `build()` |

### Built-in Strategies

| 类型 | 接口 |
|------|------|
| `RoundRobinSelector` | `ISpeakerSelector` |
| `LLMCoordinatorSelector` | `ISpeakerSelector` |
| `FixedOrderSelector` | `ISpeakerSelector` |
| `MaxRoundsTermination` | `ITerminationCondition` |
| `KeywordTermination` | `ITerminationCondition` |
| `MajorityAggregator` | `IVoteAggregator` |
| `UnanimousAggregator` | `IVoteAggregator` |
| `WeightedAggregator` | `IVoteAggregator` |

### 图与执行器

| 类型 | 字段/描述 |
|------|---------|
| `WorkflowGraph` | `nodes`, `edges`, `ports`, `output_node_ids`, `start_node_id` |
| `Node` | `id`, `executor`, `is_output`, `retry`, `timeout`, `loop_config` |
| `Edge` | 枚举 `Direct / FanOut / FanIn` |
| `LoopConfig` | `max_iterations`, `loop_variable` |
| `AgentExecutor` | 包装 `IAgent` 为 `IExecutor` |
| `FunctionExecutor<F, I, O>` | 纯函数节点，泛型参数 |
| `HumanTaskExecutor` | 暂停工作流等待外部输入 |
| `SubFlowExecutor` | 运行时构造子图执行 |
| `CompensableExecutor` | Saga 补偿包装 |

### 引擎与配置

| 类型 | 核心方法 |
|------|---------|
| `WorkflowEngine` | `new()`, `with_checkpoint_manager()`, `with_config()`, `run()`, `inject_event()` |
| `WorkflowConfig` | `new()`, `with_global_timeout()`, `with_node_timeout()`, `with_max_parallel()` |
| `WorkflowRuntime` | `start()`, `events()`, `outputs()`, `resume()`, `wait()` |
| `RetryConfig` | `max_retries`, `backoff`, `retry_on`, `on_exhausted` |
| `EventBus` | `new()`, `publish()`, `subscribe()` |
| `ExternalEvent` | `MessageReceived / SignalReceived / TimerElapsed` |
| `WorkflowEvent` | 全生命周期事件枚举（`WorkflowStarted` ~ `WorkflowCompleted`） |

### 条件系统

| 类型 | 用途 |
|------|------|
| `VariableCondition` | 单变量比较（Eq/Neq/Gt/Gte/Lt/Lte/Contains/StartsWith） |
| `ExpressionCondition` | 多条件组合（AllOf/AnyOf） |
| `VariableEdgeCondition` | IEdgeCondition 实现，从 state_map 读取 |
| `HandoffEdgeCondition` | Handoff 专用条件，匹配 triage 输出 |

### 检查点

| 类型 | 描述 |
|------|------|
| `CheckpointManager` | `with_default_config()`, `create_initial()`, `commit()`, `load_full_state()`, `cleanup()` |
| `InMemoryCheckpointStore` | 内存存储 |
| `FileCheckpointStore` | 文件持久化存储 |
| `CheckpointConfig` | `full_snapshot_interval`, `max_checkpoints`, `enabled` |

## rust-agent-decl

声明式配置 Crate。

### 文档类型

| 类型 | 方法 |
|------|------|
| `AgentDocument` | `from_json_str()`, `from_yaml_str()`, `from_toml_str()`, `from_json_file()`, `inner_definition()` |
| `AgentManifest` | `name`, `template`, `parameters`, `resources` |
| `AgentDefinition` | `name`, `description`, `kind_data`, `input_schema`, `output_schema` |

### 便利函数

| 函数 | 说明 |
|------|------|
| `quick_agent(path)` | 从文件快速构建 Agent |
| `quick_workflow(path)` | 从文件快速构建 Workflow |

### 解析器

| 类型 | 方法 |
|------|------|
| `AgentResolver` | `resolve()`, `get_agent()`, `register_tool_factory()` |
| `ToolResolver` | `resolve()`, `resolve_all()`, `register_factory()` |
| `WorkflowResolver` | `resolve_workflow()` |

## rust-agent-rhai

Rhai 脚本引擎。

| 类型 | 方法 |
|------|------|
| `RhaiRuntime` | `new()`, `with_script()`, `with_variable()`, `with_json_variable()`, `run()`, `eval()`, `eval_expression()` |
| `RhaiExecutor` | `new()`, `with_runtime()` (impl IExecutor) |
| `RhaiTool` | `new()`, `with_runtime()`, `from_script_file()` (impl ITool) |

## rust-agent-host

宿主服务。

| 类型 | 方法 |
|------|------|
| `AgentRegistry` | `register()`, `get()`, `resolve_agent()`, `build_agent_list()`, `get_subagent_tree()` |
| `SessionBridge` | `create_session()`, `get_or_create_raf_session()`, `cancel_session()` |
| `SubAgentStatusTracker` | `register()`, `ensure_active()`, `mark_completed()`, `build_status_meta()` |
| `AgentFactory` | `new()`, `create_all()`, `create_coding_agent()`, ... |
| `HostConfig` | `mode`, `ws_bind`, `provider`, `agents`, `agents_dir` |

## rust-agent-websearch

| 类型 | 说明 |
|------|------|
| `WebSearch` | ITool 实现，网络搜索 |
| `WebFetch` | ITool 实现，网页获取 |
| `WebSearchContextProvider` | IContextProvider 实现，自动搜索 |

## rust-agent-rag

| Trait | 方法 |
|-------|------|
| `DocumentLoader` | `load()`, `supported_sources()` |
| `Chunker` | `chunk()`, `strategy()` |
| `IEmbeddingModel` | `embed()`, `dimension()`, `model_id()` |
| `IVectorStore` | `store()`, `search()`, `delete()` |
| `IRetriever` | `retrieve()` |
