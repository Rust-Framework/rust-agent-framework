# Coding Crate 完成与验证计划

## 摘要

本计划旨在完成 `rust-agent-coding` crate（6 阶段自动化软件开发智能体编排）的收尾工作。基于前序会话，所有源文件和测试文件已创建完成，但存在一个明确的测试 bug（节点命名不一致）和待执行的验证步骤（编译、测试、clippy、fmt）。本计划聚焦于修复已知问题并通过全部验证。

## 当前状态分析

### 已完成的文件（11 个）

| 文件 | 行数 | 状态 |
|---|---|---|
| `crates/coding/Cargo.toml` | 25 | 完成 |
| `crates/coding/src/lib.rs` | 76 | 完成 |
| `crates/coding/src/pipeline.rs` | 282 | 完成 |
| `crates/coding/src/executors.rs` | 258 | 完成 |
| `crates/coding/src/state.rs` | 103 | 完成 |
| `crates/coding/src/conditions.rs` | 45 | 完成（文档注释轻微过时） |
| `crates/coding/src/agents.rs` | 336 | 完成 |
| `crates/coding/src/bin/coding.rs` | 135 | 完成 |
| `crates/coding/tests/pipeline_build.rs` | 90 | 完成（3 测试） |
| `crates/coding/tests/hitl_confirm.rs` | 175 | **含 bug**（2 测试） |
| `crates/coding/tests/parallel_coding.rs` | 167 | 完成（1 测试） |
| `crates/coding/tests/feedback_loop.rs` | 195 | 完成（3 测试） |

### 已确认的 Bug

**`tests/hitl_confirm.rs` 第二个测试 `test_human_task_yields_payload`（第 104-175 行）节点命名不一致：**

- 第 121 行：节点注册为 `"output"`
- 第 127 行：`.add_edge("approval", "after")` — 引用不存在的 `"after"`
- 第 128 行：`.with_output_from("after")` — 引用不存在的 `"after"`

对比同文件第一个测试 `test_human_task_halt_and_resume`（第 41-42 行）正确使用 `"output"`，确认 `"after"` 是笔误。

### 文档注释过时（非阻塞）

`src/conditions.rs` 的文档注释描述的是"广播模式"（条件边 + 无条件回边 + feedback_filter），但 `pipeline.rs` 实际采用 `review_gateway` 智能网关模式。`ReviewPassedCondition` 当前未被 pipeline 使用，属于备用实现。代码可正常编译，仅文档注释与实际不符。

### 关键 API 确认（来自框架探索）

- `WorkflowBuilder::build()` 返回 `Result<WorkflowGraph>`，会校验边端点节点存在性
- `HumanTaskExecutor` resume 时：注入 `String` → 返回 `Value::String`；注入 `Value` → 直接返回
- `FunctionExecutor` 闭包是同步的 `Fn(I) -> O`，`I` 必须实现 `Clone`
- `ContextFunctionExecutor` 闭包是异步的，返回 `Result<HandlerResult>`
- `HandlerResult` 变体：`Messages(Vec<Arc<dyn Any>>)`、`Output(Arc<dyn Any>)`、`None`
- `IEdgeCondition` 不在根导出，需用 `rust_agent_workflow::graph::IEdgeCondition`

## 提议的变更

### 变更 1：修复 hitl_confirm.rs 节点命名 bug

**文件**：`crates/coding/tests/hitl_confirm.rs`

**修改内容**：将第 127、128 行的 `"after"` 改为 `"output"`，与第 121 行的节点注册名一致。

**修改前**：
```rust
.add_edge("approval", "after")
.with_output_from("after")
```

**修改后**：
```rust
.add_edge("approval", "output")
.with_output_from("output")
```

**原因**：`build()` 会校验边端点节点存在性，引用不存在的 `"after"` 会导致 `build()` 返回 Err，`.expect("build graph")` panic，测试失败。

### 变更 2：清理 hitl_confirm.rs 中的 debug 语句（如存在）

**文件**：`crates/coding/tests/hitl_confirm.rs`

**修改内容**：检查并移除第一个测试 `test_human_task_halt_and_resume` 中遗留的 `eprintln!` 调试语句（前序会话中为排查 resume 时机问题添加）。

**原因**：调试语句不应保留在最终测试代码中。

### 变更 3：更新 conditions.rs 文档注释（可选，低优先级）

**文件**：`crates/coding/src/conditions.rs`

**修改内容**：更新模块级文档注释，说明 `ReviewPassedCondition` 是备用实现，当前 pipeline 实际采用 `review_gateway` 智能网关模式（见 `executors::review_gateway`）。

**原因**：消除文档与实际实现的不一致，避免后续维护者困惑。保留 `ReviewPassedCondition` 代码作为可选的替代方案。

## 验证步骤

按以下顺序执行验证，每一步通过后再进行下一步：

### 步骤 1：编译检查
```powershell
cargo build -p rust-agent-coding
```
预期：编译成功，无错误。

### 步骤 2：运行全部测试
```powershell
cargo test -p rust-agent-coding
```
预期：9 个测试全部通过：
- `pipeline_build.rs`：3 个（builds_successfully、has_all_stage_nodes、has_loop_config_on_p4a_inject）
- `hitl_confirm.rs`：2 个（halt_and_resume、yields_payload）
- `parallel_coding.rs`：1 个（fanout_fanin_parallel_coding）
- `feedback_loop.rs`：3 个（passes_on_approved、loops_on_rejected、verdict_parsing）

### 步骤 3：Clippy 检查
```powershell
cargo clippy -p rust-agent-coding -- -D warnings
```
预期：无警告。若有警告，逐项修复。

### 步骤 4：格式检查
```powershell
cargo fmt -p rust-agent-coding -- --check
```
预期：无格式问题。若有问题，运行 `cargo fmt -p rust-agent-coding` 自动修复后重新检查。

## 假设与决策

### 假设
1. 前序会话创建的所有源文件（除 hitl_confirm.rs 的已知 bug 外）逻辑正确，仅需通过验证确认。
2. `ReviewPassedCondition` 保留作为备用实现，不删除（避免过度清理，保留可选方案）。
3. 测试中不依赖真实 LLM API 调用（使用 mock 或简单 FunctionExecutor 模拟）。

### 决策
1. **修复策略**：最小化修改，仅修复明确的 bug，不重构已有代码。
2. **conditions.rs 处理**：更新文档注释而非删除，保留 `ReviewPassedCondition` 作为可选替代方案。
3. **验证顺序**：编译 → 测试 → clippy → fmt，确保每一步通过后再进行下一步。
4. **失败处理**：若某步骤失败，分析根因后修复，不跳过后续步骤。

## 风险与缓解

| 风险 | 缓解措施 |
|---|---|
| 磁盘空间不足（前序会话曾遇到 os error 112） | 若编译失败提示磁盘空间，先 `cargo clean` 清理其他 crate 产物 |
| 测试因 LLM API 依赖失败 | 测试应使用 mock/简单执行器，不依赖真实 API；若发现依赖，需调整测试 |
| clippy 报告大量警告 | 逐项修复，优先处理正确性警告，其次风格警告 |
| 框架 API 变更导致编译失败 | 以探索结果中的 API 签名为准，必要时调整代码 |
