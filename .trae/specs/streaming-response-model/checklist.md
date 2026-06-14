# Checklist

## ResponseMetadata
- [ ] `ResponseMetadata` 包含 `agent_id`, `model_id`, `executor_id`, `timestamp`, `properties`
- [ ] 所有 Content 变体的 `meta` 字段类型为 `ResponseMetadata`
- [ ] 所有 Event 变体的 `meta` 字段类型为 `ResponseMetadata`
- [ ] `HasMeta` trait 已实现（`fn meta(&self) -> &ResponseMetadata`）
- [ ] `properties` 从 `AgentRunOptions.properties` 透传到每个 content/event

## Content / Event 类型
- [ ] `Content` 枚举含 7 个变体：`Text`, `Reasoning`, `Uri`, `ToolCalling`, `ToolCalled`, `Usage`, `Error`
- [ ] `ToolCallingContent` 含 `call_id`, `name`, `arguments: Value`（完整 args）
- [ ] `ToolCalledContent` 含 `call_id`, `result`, `error`
- [ ] `Event` 枚举含 `ExecutorInvoking`, `ExecutorInvoked`, `Custom`
- [ ] `AgentResponseResult` 含 `id`, `model`, `finish_reason`, `contents`, `events`

## 架构分层
- [ ] `AgentResponseUpdate` 为 `pub(crate)` — 仅 client + framework crate 可见
- [ ] `AgentResponseConverter` 独立于 `crates/framework/src/converter.rs`
- [ ] Transport 层（client）不引用 `AgentResponseResult` 或 `Content`/`Event`
- [ ] Conversion 层（framework）不包含 SSE 解析逻辑
- [ ] Public API（core）不引用 `AgentResponseUpdate`

## AgentResponseConverter
- [ ] `consume(TextDelta)` → `Content::Text` + 正确 `ResponseMetadata`
- [ ] `consume(ToolCallDelta)` → 按 index 累积 → args 完整时产出 `Content::ToolCalling`
- [ ] `consume(Usage)` → `Content::Usage`
- [ ] `consume(Finish)` → 记录 pending finish_reason
- [ ] `consume(ResponseMetadata)` → 记录 id/model
- [ ] `consume(Error)` → `Content::Error`
- [ ] `finalize()` → 产出含 finish_reason 的最终 `AgentResponseResult`

## IAgent 统一门面
- [ ] `AgentBuilder::build() → Arc<dyn IAgent>`
- [ ] `WorkflowBuilder::build() → Arc<dyn IAgent>`
- [ ] `IAgent::get_subagent()` 正确实现（单Agent→None, ToolLoop→inner, Workflow→按executor查找）
- [ ] `AgentBuilder::with_properties()` 透传到 `AgentRunOptions.properties`

## AgentRunOptions
- [ ] 含 `properties: HashMap<String, Value>` 字段
- [ ] `properties` 经由 `AgentResponseConverter` 透传到每个 `ResponseMetadata`

## 消息类型完整性
- [ ] `ChatMessage` 含 `tool_calls` 和 `tool_call_id`
- [ ] Assistant 消息含 `tool_calls` 时正确序列化
- [ ] Tool 消息含 `tool_call_id` 时正确序列化
- [ ] `AgentResponse` 含 `id`, `model`, `reasoning_text`, `finish_reason`, `usage`
- [ ] `Usage` 含 `prompt_cache_hit_tokens`, `prompt_cache_miss_tokens`
- [ ] `CollectAgentResponse` 消费 `BoxStream<AgentResponseResult>`

## Session 管理
- [ ] `ISession` trait 含 `session_id()`, `add_message()`, `get_messages()`, `clear()`, `metadata()`, `snapshot()`, `serialize()`, `deserialize()`
- [ ] `SessionMetadata` 含 `created_at`, `updated_at`, `message_count`, `last_request_hash`
- [ ] `SessionSnapshot` 含 `session_id`, `metadata`, `messages: Vec<ChatMessage>`
- [ ] `AgentSession::new()` 使用 `Uuid::new_v4()` 生成 session_id（非全局计数器）
- [ ] `AgentSession::add_message()` 自动更新 `updated_at` 和 `message_count`
- [ ] `AgentSession::clear()` 自动更新 `updated_at`，重置 `message_count` 为 0
- [ ] `AgentSession::touch_request_hash()` 对 messages 内容计算哈希并写入 `last_request_hash`
- [ ] `AgentSession::serialize()` → JSON（通过 `SessionSnapshot` 中转）
- [ ] `AgentSession::deserialize(json)` → 完整恢复
- [ ] 全局 `SESSION_COUNTER: AtomicU64` 已移除
- [ ] `HistoryAgent` 正确实现：加载历史 → 拼装 `[system]+history+[new]` → inner.run → 回写 session
- [ ] KV 缓存规则保证：system 不变 / 前缀递增 / 不删不改中间消息 / 拼装顺序固定
- [ ] 序列化 JSON 格式与 spec 一致

## 编译验证
- [ ] `cargo check --workspace` 零 error
- [ ] `cargo build --workspace` 零 error
- [ ] `cargo test --workspace` 全部通过
- [ ] 无 warning
