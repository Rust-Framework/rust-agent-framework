# Tasks

## Task 1: 核心类型体系重构（core crate）
- [ ] 1.1 新增 `ResponseMetadata` 结构体到 `core/src/types.rs`（含 `agent_id`, `model_id`, `executor_id`, `timestamp`, `properties`）
- [ ] 1.2 新增 `FinishReason` 枚举到 `core/src/types.rs`
- [ ] 1.3 新增 `Usage` 结构体到 `core/src/types.rs`（从 client 提升 `UsageStats` 并重命名）
- [ ] 1.4 新增 `Content` 枚举到 `core/src/message.rs`，含 7 个变体：`TextContent`, `ReasoningContent`, `UriContent`, `ToolCallingContent`, `ToolCalledContent`, `UsageContent`, `ErrorContent`；每个变体字段含 `meta: ResponseMetadata`
- [ ] 1.5 新增 `HasMeta` trait（`fn meta(&self) -> &ResponseMetadata`），为所有 Content/Event 变体实现
- [ ] 1.6 新增 `Event` 枚举到 `core/src/message.rs`，含 `ExecutorInvokingEvent`, `ExecutorInvokedEvent`, `CustomEvent`
- [ ] 1.7 新增 `AgentResponseResult` 结构体：`id`, `model`, `finish_reason`, `contents: Vec<Content>`, `events: Vec<Event>`
- [ ] 1.8 新增 `AgentResponseUpdate` 枚举为 `pub(crate)` 内部类型（供 client + framework crate 使用）
- [ ] 1.9 扩展 `ChatMessage`：新增 `tool_calls`, `tool_call_id`
- [ ] 1.10 扩展 `AgentResponse`：新增 `id`, `model`, `reasoning_text`, `finish_reason`, `usage`
- [ ] 1.11 重命名 `ChatAgentRunOptions` → `AgentRunOptions`，新增 `properties: HashMap<String, Value>` 字段
- [ ] 1.12 扩展 `IAgent` trait：`run()` 返回 `BoxStream<AgentResponseResult>`；新增 `get_subagent()`, `list_subagents()`
- [ ] 1.13 更新 `core/src/lib.rs` 导出所有新类型
- [ ] 1.14 移除旧的 `ChatStreamChunk`、`AgentStreamChunk`、`ToolCallDelta`、`ChatClientRunOptions`

## Task 2: Session 管理增强（core crate）
- [ ] 2.1 新增 `SessionMetadata` 到 `core/src/session.rs`（`created_at`, `updated_at`, `message_count`, `last_request_hash`）
- [ ] 2.2 新增 `SessionSnapshot` 到 `core/src/session.rs`（`session_id`, `metadata`, `messages`）
- [ ] 2.3 扩展 `ISession` trait：新增 `metadata()`, `snapshot()`, `serialize()`, `deserialize()`
- [ ] 2.4 重构 `AgentSession`：
  - [ ] 移除全局 `SESSION_COUNTER: AtomicU64`
  - [ ] session_id 使用 `Uuid::new_v4()` 生成
  - [ ] 新增 `created_at`/`updated_at` 时间戳（`add_message()` / `clear()` 时自动更新）
  - [ ] 新增 `last_request_hash` 字段 + `touch_request_hash()` 方法（xxhash 对 messages 内容哈希）
  - [ ] 实现 `serialize()` → JSON（通过 `SessionSnapshot`）
  - [ ] 实现 `deserialize(json)` → 恢复 `AgentSession`
  - [ ] 实现 `snapshot()` → 只读 `SessionSnapshot`
- [ ] 2.5 添加依赖到 `core/Cargo.toml`：`chrono`（`DateTime<Utc>`）、`uuid`（`Uuid::new_v4()`）

## Task 3: Transport 层 — SSE 解析器重写（client crate）
- [ ] 3.1 扩展 `SseChunk`：新增 `id`, `object`, `model`, `usage`, `system_fingerprint`, `finish_reason` 字段
- [ ] 3.2 重写 `map_chunk()` → 返回 `Vec<AgentResponseUpdate>`（内部类型）
- [ ] 3.3 更新 `SseStream` 返回 `Stream<Item = Result<AgentResponseUpdate, AgentError>>`
- [ ] 3.4 更新 `IChatClient::run()` 返回类型
- [ ] 3.5 移除 `UsageStats`/`CacheHitInfo`（已提升到 core）

## Task 4: Conversion 层 — AgentResponseConverter（framework crate）
- [ ] 4.1 新增 `AgentResponseConverter` 结构体到 `crates/framework/src/converter.rs`
  - [ ] `new(agent_id, executor_id, options: &AgentRunOptions)`
  - [ ] `build_meta() → ResponseMetadata`（填充 agent_id/model_id/executor_id/timestamp/properties）
  - [ ] `consume(update: AgentResponseUpdate) → ConvertOutput`
    - [ ] `TextDelta` → `Content::Text(TextContent { meta, delta })`
    - [ ] `ReasoningDelta` → `Content::Reasoning(ReasoningContent { meta, delta })`
    - [ ] `ToolCallDelta` → 按 index 累积到 `HashMap<usize, ToolCallAccumulator>`；当 id/name/arguments_delta 完整时 → `Content::ToolCalling`
    - [ ] `Usage` → `Content::Usage(UsageContent { meta, usage })`
    - [ ] `Error` → `Content::Error(ErrorContent { meta, error_code, message })`
    - [ ] `ResponseMetadata { id, model }` → 记录到内部 `response_id`/`response_model`
    - [ ] `Finish` → 记录 `pending_finish_reason` + `pending_usage`
  - [ ] `finalize() → AgentResponseResult`：产出最终 chunk（含 finish_reason + 所有累积的 content）
- [ ] 4.2 `ConvertOutput { contents: Vec<Content>, events: Vec<Event> }`

## Task 5: ToolLoopAgent — 工具两阶段（framework crate）
- [ ] 5.1 新增 `crates/framework/src/agents/tool_loop_agent.rs`：实现 `IAgent`
- [ ] 5.2 消费 `inner.run()` → `Stream<AgentResponseResult>`
- [ ] 5.3 拦截 `Content::ToolCalling` → 产出 `Event::ExecutorInvoking` → `ITool.execute()` → 产出 `Content::ToolCalled { call_id, result/error }` → `Event::ExecutorInvoked`
- [ ] 5.4 构造 `ChatMessage::tool()` 回传 inner agent，循环直到 `Finish(Stop)` 或 `max_rounds`
- [ ] 5.5 `get_subagent()` → 返回 inner agent

## Task 6: ChatClientAgent 适配（framework crate）
- [ ] 6.1 重构 `chat_client_agent.rs` 实现 `IAgent`：`run()` 内部使用 `AgentResponseConverter` 将 `Stream<AgentResponseUpdate>` 转为 `Stream<AgentResponseResult>`
- [ ] 6.2 消息拼接：Assistant 含 `tool_calls`，Tool 含 `tool_call_id`
- [ ] 6.3 `get_subagent()` → `None`（无子代理）

## Task 7: IAgent 统一门面（framework + workflow crate）
- [ ] 7.1 `AgentBuilder::build()` → 组装管道 `TracingAgent(ToolLoopAgent(HistoryAgent(ChatClientAgent)))` → `Arc<dyn IAgent>`
- [ ] 7.2 确保 `ToolLoopAgent` / `HistoryAgent` / `TracingAgent` 都实现 `IAgent`（含 `get_subagent`）
- [ ] 7.3 Workflow crate 中确保 `GraphFlow`/`Workflow` 实现 `IAgent`（含 `get_subagent`/`list_subagents`）

## Task 8: 流聚合器重写（core crate）
- [ ] 8.1 重写 `collect_agent_response()` 消费 `BoxStream<AgentResponseResult>`
  - [ ] 拼接 `TextContent.delta` → `text`
  - [ ] 拼接 `ReasoningContent.delta` → `reasoning_text`
  - [ ] 收集 `ToolCallingContent` → `Vec<ToolCall>`
  - [ ] 提取 `UsageContent` → `Usage`
  - [ ] 提取 `AgentResponseResult.finish_reason`

## Task 9: CLI 适配（cli crate）
- [ ] 9.1 渲染 `AgentResponseResult`：contents + events 双路消费
- [ ] 9.2 渲染 `ToolCalling` / `ToolCalled` 两阶段输出
- [ ] 9.3 渲染 `ResponseMetadata.properties` 透传信息
- [ ] 9.4 渲染 `Usage` 时输出 KV 缓存命中率

## Task 10: 编译验证
- [ ] 10.1 `cargo check --workspace` 通过
- [ ] 10.2 `cargo test --workspace` 通过
- [ ] 10.3 `cargo build` 无 warning

# Task Dependencies
- Task 2 依赖 Task 1
- Task 3 依赖 Task 1
- Task 4 依赖 Task 1, 3
- Task 5 依赖 Task 1, 4
- Task 6 依赖 Task 1, 3, 4
- Task 7 依赖 Task 5, 6
- Task 8 依赖 Task 1
- Task 9 依赖 Task 6
- Task 10 依赖所有前序 Task
