# Rhai Executor 实现计划

## 概要

新建 `crates/rhai` crate（包名 `rust-agent-rhai`），基于 Rhai 嵌入式脚本引擎为 workflow 体系提供动态嵌入式脚本语言支持。核心交付物：

1. **RhaiRuntime** — 高内聚低耦合的 Rhai 运行时环境，统一管理脚本引擎、作用域、动态模块注册
2. **RhaiExecutor** — 实现 `IExecutor` trait，可直接作为 workflow 节点使用
3. **RhaiTool** — 实现 `ITool` trait，供智能体通过 ToolRegistry 调用 Rhai 脚本

---

## 当前状态分析

### 工作区约定

| 约定 | 说明 |
|------|------|
| Crate 命名 | `rust-agent-<module>` 格式（如 `rust-agent-workflow`, `rust-agent-core`） |
| 目录结构 | `<workspace>/crates/<name>/`，含 `src/lib.rs` 和 `Cargo.toml` |
| 版本管理 | `version.workspace = true`，`edition.workspace = true` |
| 依赖管理 | 共享依赖通过 `[workspace.dependencies]` 定义 |

### 现有 Executor 模式（workflow crate）

- **`IExecutor` trait**（[base.rs](file:///d:/GitCode/RF/rust-agent-framework/crates/workflow/src/executor/base.rs)）：定义 `id()`、`accepted_types()`、`send_types()`、`handle()` 及生命周期钩子
- **`AgentExecutor`**（[agent_executor.rs](file:///d:/GitCode/RF/rust-agent-framework/crates/workflow/src/executor/agent_executor.rs)）：将 `IAgent` 适配为 `IExecutor`，含全链路流式转发
- **`FunctionExecutor`**（[function_executor.rs](file:///d:/GitCode/RF/rust-agent-framework/crates/workflow/src/executor/function_executor.rs)）：泛型轻量 Executor，用于纯逻辑节点
- **`HandlerResult`**：`Messages` / `Output` / `None` 三种返回模式

### 现有 Tool 模式（core crate）

- **`ITool` trait**（[tool.rs](file:///d:/GitCode/RF/rust-agent-framework/crates/core/src/tool.rs)）：`name()`、`description()`、`parameters()`、`execute(arguments) -> Result<String>`
- **`ToolRegistry`**：HashMap 管理工具注册与查询

### 现有错误处理

- `AgentError` 枚举（[error.rs](file:///d:/GitCode/RF/rust-agent-framework/crates/core/src/error.rs)），含 `ChatClientError`、`ToolError`、`WorkflowError` 等变体，使用 `thiserror`

---

## 提议变更

### 1. 新增 `crates/rhai/` 目录及文件

创建以下文件：

```
crates/rhai/
├── Cargo.toml
└── src/
    ├── lib.rs           # crate 入口，模块声明 + 重导出
    ├── runtime.rs       # RhaiRuntime — 封装 rhai::Engine + Scope + 模块注册
    ├── executor.rs      # RhaiExecutor — IExecutor 实现
    └── tool.rs          # RhaiTool — ITool 实现
```

### 2. 修改 `Cargo.toml`（workspace 根）

- `[workspace].members` 新增 `"crates/rhai"`
- `[workspace.dependencies]` 新增：
  - `rust-agent-rhai = { path = "crates/rhai", version = "0.1.0" }`
  - `rhai = "1"`

### 3. `crates/rhai/Cargo.toml`

```toml
[package]
name = "rust-agent-rhai"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
homepage.workspace = true
description = "Rhai scripting engine integration — dynamic embedded scripting for workflow nodes and agent tools"

[dependencies]
rust-agent-core = { workspace = true }
rust-agent-workflow = { workspace = true }
rhai = { workspace = true }
tokio = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
async-trait = { workspace = true }
tracing = { workspace = true }
```

### 4. `src/lib.rs`

模块声明与公共 API 重导出：

```rust
pub mod runtime;
pub mod executor;
pub mod tool;

// 重导出核心类型
pub use runtime::RhaiRuntime;
pub use executor::RhaiExecutor;
pub use tool::RhaiTool;
```

### 5. `src/runtime.rs` — RhaiRuntime 设计

**设计目标**：高内聚 — 脚本引擎、作用域、模块注册统一管理；低耦合 — 通过 trait 和回调注入外部依赖。

**核心结构**：

```rust
pub struct RhaiRuntime {
    engine: rhai::Engine,
    scope: rhai::Scope<'static>,
    script_name: String,
    // 可通过回调注入的上下文数据
    on_call: Option<Box<dyn Fn(&str, &[rhai::Dynamic]) -> rhai::Dynamic + Send + Sync>>,
}
```

**关键方法**：

| 方法 | 说明 |
|------|------|
| `new()` | 创建带默认配置的运行时（沙箱模式、无标准库、操作数限制） |
| `with_script(script)` | 设置要执行的 Rhai 脚本 |
| `with_variable(name, value)` | 注入变量到作用域 |
| `with_module(name, module)` | 注册自定义模块（静态函数/类型） |
| `with_dynamic_module(name, callbacks)` | 注册动态模块（支持运行时回调） |
| `set_context_provider(fn)` | 设置上下文数据注入回调 |
| `run()` | 执行脚本并返回 `serde_json::Value` |
| `eval_expression<T>(expr)` | 快速求值单行表达式 |
| `compile(script)` | 预编译为 `rhai::AST` 复用 |

**安全策略**：
- `Engine::new_raw()` — 禁用标准库
- `set_max_operations(max_ops)` — 防止无限循环，默认 100_000
- 禁用 `eval`、`import` 等危险内置函数（如需要）
- 通过模块系统白名单注册能力

### 6. `src/executor.rs` — RhaiExecutor 设计

实现 `IExecutor` trait，作为 workflow 节点：

```rust
pub struct RhaiExecutor {
    id: String,
    runtime: Arc<RhaiRuntime>,
    script_source: String,
    // 输入变量映射
    input_var: String,   // 从 message 中提取并绑定到此变量的名称
}
```

**`IExecutor::handle()` 逻辑**：

1. 从 `message` 中提取输入数据（`downcast` 尝试 `serde_json::Value`，回退到 `String`）
2. 将输入注入 `scope` 作为变量（变量名由 `input_var` 配置）
3. 将 `IWorkflowContext` 状态读写能力注入作用域（`ctx_read` / `ctx_write` / `ctx_node_id`）
4. 通过 `NodeProgress` channel 注入回调（`emit_text` / `emit_custom`）
5. 调用 `runtime.run()` 执行脚本
6. 结果包装为 `HandlerResult::Messages(vec![json_value])`

**进度集成**：Rhai 脚本中可调用 `emit_text("...")` 向 workflow 引擎推送流式事件。

### 7. `src/tool.rs` — RhaiTool 设计

实现 `ITool` trait，作为 Agent 可调用工具：

```rust
pub struct RhaiTool {
    name: String,
    description: String,
    parameters: serde_json::Value,
    runtime: Arc<RhaiRuntime>,
    script_source: String,
}
```

**`ITool::execute(arguments)` 逻辑**：

1. 将 `arguments` 注入 scope 作为 `args` 变量
2. 执行脚本
3. 返回结果（转为 JSON 字符串）

这样 Agent 可以通过 tool calling 触发 Rhai 脚本，实现灵活的运行时逻辑。

**辅助构造器**：`RhaiTool::from_script_file(name, description, schema, script_path)` 从文件加载脚本。

### 8. Builder 模式（可选增强）

为 `RhaiRuntime` 提供 builder 模式链式 API：

```rust
let runtime = RhaiRuntime::builder()
    .sandboxed(true)
    .max_operations(50_000)
    .with_variable("config", config_json)
    .with_script(r#" ... "#)
    .build()?;
```

---

## 假设与决策

| # | 决策 | 理由 |
|---|------|------|
| 1 | 使用 `rhai = "1"`（最新稳定版） | 社区活跃，API 稳定，性能良好 |
| 2 | Engine 使用 `new_raw()` 沙箱模式 | 安全性要求，避免脚本执行任意系统调用 |
| 3 | 不依赖 `rust-agent-framework`，仅依赖 `rust-agent-core` + `rust-agent-workflow` | 最小化依赖，低耦合 |
| 4 | RhaiExecutor 的 `send_types` / `accepted_types` 使用 `serde_json::Value` 类型标签 | 与 FunctionExecutor 一致的类型擦除模式 |
| 5 | 进度回调通过注册 Rhai 函数实现（`emit_text`, `emit_custom`） | 无需侵入 IExecutor 接口 |
| 6 | `RhaiTool` 不需要流式进度（Tool 接口无此概念） | 保持 ITool 接口简洁 |

---

## 验证步骤

1. `cargo check -p rust-agent-rhai` — 编译通过
2. `cargo clippy -p rust-agent-rhai` — 无 lint 警告
3. `cargo test -p rust-agent-rhai` — 单元测试通过（如有测试）
4. `cargo build --workspace` — 新 crate 不破坏现有 workspace 编译
5. 手动验证：RhaiExecutor 在 workflow 中的基本执行、RhaiTool 通过 ToolRegistry 调用

---

## 关键文件清单

| 文件 | 操作 | 说明 |
|------|------|------|
| `Cargo.toml`（根） | 编辑 | 添加 workspace member + rhai dependency |
| `crates/rhai/Cargo.toml` | 新建 | Crate 配置 |
| `crates/rhai/src/lib.rs` | 新建 | 模块声明 + 重导出 |
| `crates/rhai/src/runtime.rs` | 新建 | RhaiRuntime 核心实现 |
| `crates/rhai/src/executor.rs` | 新建 | IExecutor 实现 |
| `crates/rhai/src/tool.rs` | 新建 | ITool 实现 |
