# rust-agent-decl 声明式包设计计划

## 一、概述

在 `crates/decl/` 下创建新 crate `rust-agent-decl`，为 `rust-agent-framework` 和 `rust-agent-workflow` 提供**声明式构建能力**。用户可以通过 JSON / YAML / TOML 数据文件定义 Agent 和 Workflow 的完整配置，无需编写 Rust 代码即可组装 Agent 与工作流。

## 二、当前状态分析

### 2.1 现有架构

- **`rust-agent-core`**：定义核心 trait（`IAgent`, `IChatClient`, `ITool`, `ISession`, `IContextProvider` 等），几乎所有核心类型均已 `#[derive(Serialize, Deserialize)]`。
- **`rust-agent-framework`**：提供 `AgentBuilder<C>`（泛型 Builder）、`ChatClientAgent`（`IAgent` 的具体实现）、`ToolRegistry`、`AgentRuntime`、`AgentHost` 和内置工具。
- **`rust-agent-workflow`**：提供 `WorkflowBuilder`、`WorkflowGraph`、`Node`、`Edge`、`IExecutor`、`IWorkflowContext` 及编排模式。
- **`rust-agent-rhai`**：Rhai 脚本集成，将脚本封装为 `ITool` / `IExecutor` — 是声明式扩展的现有先例。

### 2.2 关键约束

| 约束 | 说明 |
|------|------|
| `IChatClient` 是 trait object | 无法直接反序列化，需要**工厂/解析器模式** |
| `ITool` / `IContextProvider` / `ICompressionStrategy` 是 trait object | 同上 |
| `WorkflowGraph` 不可直接序列化 | 含 `Arc<dyn IExecutor>` trait object |
| `Edge` 枚举不可直接序列化 | 含 `Box<dyn IEdgeCondition>` 等 |

### 2.3 核心洞察

`rust-agent-decl` 应定义**数据模型层**（可序列化的"配方"），再通过**解析器（Resolver）**将配方转换为运行时对象。这与 `AgentBuilder` / `WorkflowBuilder` 的关系类似：Builder 是 Rust 代码中的"配方"，decl 是数据文件中的"配方"。

## 三、设计方案

### 3.1 Crate 结构

```
crates/decl/
├── Cargo.toml
└── src/
    ├── lib.rs              # 模块声明 + 公开导出
    ├── agent.rs            # AgentDecl 声明模型 + AgentResolver trait
    ├── workflow.rs         # WorkflowDecl 声明模型 + WorkflowResolver trait
    ├── tool.rs             # ToolDecl 声明模型（内置工具引用）
    ├── resolver.rs         # 默认解析器实现
    └── error.rs            # 错误类型
```

### 3.2 依赖关系

```
rust-agent-decl
  ├── rust-agent-core      (核心类型)
  ├── rust-agent-framework  (AgentBuilder, ChatClientAgent, 内置工具)
  ├── rust-agent-workflow   (WorkflowBuilder, AgentExecutor, FunctionExecutor)
  ├── serde                 (序列化框架)
  ├── serde_json            (JSON 支持, always)
  ├── serde_yaml            (YAML 支持, feature-gated)
  ├── toml                  (TOML 支持, feature-gated)
  └── thiserror             (错误定义)
```

Feature flags:
- `default = ["json"]`
- `json` — serde_json 支持
- `yaml` — serde_yaml 支持
- `toml` — toml 支持

### 3.3 核心类型设计

#### 3.3.1 AgentDecl（Agent 声明模型）

```rust
/// Agent 声明式定义 — 对应 AgentBuilder 的全部能力
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDecl {
    /// Agent 唯一标识
    pub id: String,
    /// Agent 描述
    #[serde(default)]
    pub description: String,
    /// 系统指令
    #[serde(default)]
    pub instructions: String,
    /// 模型配置（必填 — 由 Resolver 创建 IChatClient）
    pub model: ModelConfig,
    /// 工具声明列表
    #[serde(default)]
    pub tools: Vec<ToolRef>,
    /// 上下文提供器声明
    #[serde(default)]
    pub context_providers: Vec<ContextProviderDecl>,
    /// 额外属性
    #[serde(default)]
    pub properties: HashMap<String, serde_json::Value>,
    /// 最大工具调用轮次
    #[serde(default = "default_max_tool_rounds")]
    pub max_tool_rounds: usize,
    /// 压缩策略声明
    #[serde(default)]
    pub compression: Option<CompressionDecl>,
    /// Token 计数器声明
    #[serde(default)]
    pub token_counter: Option<TokenCounterDecl>,
    /// 运行选项覆盖
    #[serde(default)]
    pub run_options: Option<AgentRunOptions>,
    /// 子 Agent 声明（用于多 Agent 场景）
    #[serde(default)]
    pub sub_agents: Vec<AgentDecl>,
}
```

#### 3.3.2 ModelConfig（模型配置）

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    /// 提供商: "openai" | "deepseek" | "custom"
    pub provider: String,
    /// 模型名称，如 "gpt-4o", "deepseek-chat"
    pub model: String,
    /// API 密钥（或环境变量名 "$ENV_VAR"）
    #[serde(default)]
    pub api_key: Option<String>,
    /// API Base URL（可选）
    #[serde(default)]
    pub base_url: Option<String>,
    /// 默认温度
    #[serde(default)]
    pub temperature: Option<f32>,
    /// 默认 max_tokens
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// 额外 HTTP 请求头
    #[serde(default)]
    pub extra_headers: HashMap<String, String>,
    /// 扩展配置
    #[serde(default)]
    pub extra: HashMap<String, serde_json::Value>,
}
```

#### 3.3.3 ToolRef（工具引用）

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolRef {
    /// 引用内置工具（框架 tools/ 目录下的工具）
    Builtin {
        name: String,           // "read_file", "write_file", "web_search" 等
        #[serde(default)]
        config: HashMap<String, serde_json::Value>,
    },
    /// 外部 Rhai 脚本工具
    Rhai {
        name: String,
        description: String,
        /// Rhai 脚本文件路径
        script_path: String,
        /// JSON Schema 格式的参数定义
        #[serde(default)]
        parameters: serde_json::Value,
    },
    /// 自定义工具（需在 Resolver 注册工厂）
    Custom {
        name: String,
        #[serde(default)]
        config: HashMap<String, serde_json::Value>,
    },
}
```

#### 3.3.4 WorkflowDecl（工作流声明模型）

```rust
/// 工作流声明式定义 — 对应 WorkflowBuilder 的全部能力
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDecl {
    /// 工作流名称
    pub name: String,
    /// 节点列表
    pub nodes: Vec<NodeDecl>,
    /// 边列表
    pub edges: Vec<EdgeDecl>,
    /// 入口节点 ID
    pub start_node_id: String,
    /// 输出节点 ID 列表
    #[serde(default)]
    pub output_node_ids: Vec<String>,
    /// 外部请求端口
    #[serde(default)]
    pub ports: Vec<PortDecl>,
}
```

#### 3.3.5 NodeDecl（节点声明）

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NodeDecl {
    /// Agent 节点 — 引用已注册的 Agent
    Agent {
        id: String,
        /// 引用全局 Agent 注册表中的 Agent ID
        agent_ref: String,
        /// 或内联 Agent 声明
        #[serde(default)]
        agent: Option<AgentDecl>,
        #[serde(default)]
        is_output: bool,
    },
    /// 函数节点 — 注册在 Resolver 中的纯函数
    Function {
        id: String,
        /// 函数注册名
        function_ref: String,
        #[serde(default)]
        config: HashMap<String, serde_json::Value>,
        #[serde(default)]
        is_output: bool,
    },
    /// Rhai 脚本节点
    Rhai {
        id: String,
        script_path: String,
        #[serde(default)]
        is_output: bool,
    },
}
```

#### 3.3.6 EdgeDecl（边声明）

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EdgeDecl {
    /// 直接边: source -> target
    Direct {
        source: String,
        target: String,
    },
    /// 扇出边: source -> [targets...]
    FanOut {
        source: String,
        targets: Vec<String>,
    },
    /// 扇入边: [sources...] -> target
    FanIn {
        sources: Vec<String>,
        target: String,
    },
}
```

#### 3.3.7 AgentResolver / WorkflowResolver Trait

```rust
/// 将 AgentDecl 解析为可运行的 IAgent
#[async_trait]
pub trait AgentResolver: Send + Sync {
    /// 解析 AgentDecl 为 Arc<dyn IAgent>
    async fn resolve(&self, decl: &AgentDecl) -> Result<Arc<dyn IAgent>>;

    /// 解析 ToolRef 为 Arc<dyn ITool>
    async fn resolve_tool(&self, tool_ref: &ToolRef) -> Result<Arc<dyn ITool>>;

    /// 通过名称获取已注册的 Agent（用于工作流节点引用）
    fn get_agent(&self, name: &str) -> Option<Arc<dyn IAgent>>;
}

/// 将 WorkflowDecl 解析为 WorkflowGraph
#[async_trait]
pub trait WorkflowResolver: Send + Sync {
    async fn resolve(&self, decl: &WorkflowDecl) -> Result<WorkflowGraph>;
    fn resolve_node_executor(&self, node: &NodeDecl) -> Result<Arc<dyn IExecutor>>;
}
```

#### 3.3.8 默认解析器（DefaultAgentResolver）

```rust
pub struct DefaultAgentResolver {
    /// 内置工具工厂映射
    tool_factories: HashMap<String, Arc<dyn ToolFactory>>,
    /// 已解析的 Agent 注册表（按 ID）
    agent_registry: HashMap<String, Arc<dyn IAgent>>,
    /// 自定义工具工厂
    custom_tool_factories: HashMap<String, Box<dyn Fn(HashMap<String, Value>) -> Result<Arc<dyn ITool>> + Send + Sync>>,
    /// 函数节点工厂
    function_factories: HashMap<String, Arc<dyn FunctionFactory>>,
}
```

### 3.4 顶层 API

```rust
// 从文件加载
AgentDecl::from_json_file(path: &str) -> Result<AgentDecl>
AgentDecl::from_yaml_file(path: &str) -> Result<AgentDecl>   // feature = "yaml"
AgentDecl::from_toml_file(path: &str) -> Result<AgentDecl>   // feature = "toml"

// 从字符串加载
AgentDecl::from_json_str(s: &str) -> Result<AgentDecl>
AgentDecl::from_yaml_str(s: &str) -> Result<AgentDecl>
AgentDecl::from_toml_str(s: &str) -> Result<AgentDecl>

// 同样适用于 WorkflowDecl

// 一键构建
let resolver = DefaultAgentResolver::new();
let agent: Arc<dyn IAgent> = resolver.build_agent_from_file("agent.json").await?;

let workflow_resolver = DefaultWorkflowResolver::new(&agent_resolver);
let graph: WorkflowGraph = workflow_resolver.build_from_file("workflow.yaml").await?;
```

### 3.5 示例数据文件

#### agent.json

```json
{
  "id": "code-reviewer",
  "description": "Code review agent",
  "instructions": "You are a code review expert. Analyze the provided code carefully.",
  "model": {
    "provider": "openai",
    "model": "gpt-4o",
    "api_key": "$OPENAI_API_KEY",
    "temperature": 0.3
  },
  "tools": [
    { "type": "builtin", "name": "read_file" },
    { "type": "builtin", "name": "list_files" }
  ],
  "max_tool_rounds": 5
}
```

#### workflow.yaml

```yaml
name: "research-workflow"
nodes:
  - type: agent
    id: "researcher"
    agent_ref: "researcher-agent"
    is_output: false
  - type: agent
    id: "writer"
    agent_ref: "writer-agent"
    is_output: true
  - type: rhai
    id: "validator"
    script_path: "./scripts/validate.rhai"
edges:
  - type: direct
    source: "researcher"
    target: "validator"
  - type: direct
    source: "validator"
    target: "writer"
start_node_id: "researcher"
output_node_ids:
  - "writer"
```

## 四、实现步骤

### Step 1: 创建 crate 骨架

- 创建 `crates/decl/` 目录
- 编写 `Cargo.toml`，含 workspace 依赖 + feature flags
- 编写 `src/lib.rs` 模块声明
- 在根 `Cargo.toml` workspace.members 中添加 `"crates/decl"`
- 在 `[workspace.dependencies]` 中添加 `rust-agent-decl`

### Step 2: 实现错误类型 (`error.rs`)

- 定义 `DeclError` 枚举（IO 错误、解析错误、解析失败、不支持的配置等）
- 实现 `From` 转换：`serde_json::Error`、`serde_yaml::Error`、`toml` 错误、`AgentError`

### Step 3: 实现 Agent 声明模型 (`agent.rs`)

- `ModelConfig`
- `ToolRef` 枚举
- `ContextProviderDecl`
- `CompressionDecl`, `TokenCounterDecl`
- `AgentDecl`（含所有字段 + serde 属性）
- `AgentDecl` 的 `from_json_str` / `from_yaml_str` / `from_toml_str` + 文件加载方法

### Step 4: 实现 Workflow 声明模型 (`workflow.rs`)

- `NodeDecl` 枚举
- `EdgeDecl` 枚举
- `PortDecl`
- `WorkflowDecl`（含所有字段）
- 相应的加载方法

### Step 5: 实现 Resolver trait 与默认实现 (`resolver.rs`)

- `AgentResolver` trait
- `WorkflowResolver` trait
- `DefaultAgentResolver`（注册内置工具工厂：read_file、write_file、edit_file、list_files、find_files、web_search、web_fetch、run_command 等）
- `DefaultWorkflowResolver`（依赖 `AgentResolver` 获取 Agent 引用）
- 解析逻辑：`AgentDecl` -> `AgentBuilder` -> `Arc<dyn IAgent>`

### Step 6: 编写集成测试

- JSON 文件加载 Agent 测试
- YAML 文件加载 Workflow 测试
- 端到端：声明文件 -> 构建 -> 运行

## 五、假设与决策

1. **IChatClient 创建**：`DefaultAgentResolver` 基于 `ModelConfig` 创建 `rust-agent-client` 的 `OpenAIChatClient` 或 `DeepSeekChatClient`。这要求 `rust-agent-decl` 依赖 `rust-agent-client`。
2. **API Key 解析**：支持 `$ENV_VAR` 格式从环境变量读取密钥。
3. **内置工具覆盖**：首发覆盖框架中 `tools/` 下的 13 个内置工具，通过工厂模式注册。
4. **子 Agent**：`AgentDecl.sub_agents` 递归解析，子 Agent 通过 `IAgent::get_subagent()` 暴露。
5. **Feature flags**：YAML 和 TOML 为可选 feature，JSON 为默认 feature。
6. **错误处理**：使用 `thiserror` 而非 `anyhow`，与 `rust-agent-core` 的 `AgentError` 风格一致。
7. **不修改现有代码**：纯新增 crate，对现有工作区零侵入。

## 六、验证步骤

1. `cargo check -p rust-agent-decl` — 编译通过
2. `cargo clippy -p rust-agent-decl` — 无 clippy 警告
3. `cargo test -p rust-agent-decl` — 集成测试通过
4. 手动验证：用 JSON/YAML/TOML 文件定义 Agent，通过 Resolver 构建并调用 `agent.run()`
