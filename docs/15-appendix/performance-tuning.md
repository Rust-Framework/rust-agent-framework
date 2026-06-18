# 15.6 性能调优指南

本文档提供 RAF 框架的性能优化建议，涵盖会话存储、上下文压缩、工具执行、token 计数和流式处理等方面。

## 会话存储选择

### 对比分析

| 存储类型 | 延迟 | 持久化 | 适用场景 |
|---------|------|-------|---------|
| `AgentSession`（内存） | ~0μs | ❌ | 开发/测试 |
| `FileSessionStore`（文件） | ~1-10ms | ✅ | 单机小规模 |
| PostgreSQL（自定义） | ~5-20ms | ✅ | 生产环境 |
| Redis（自定义） | ~1-3ms | ✅ | 高性能 |

### 建议

```rust
// 开发环境：内存存储
let session = Arc::new(AgentSession::new());

// 生产环境：实现 ISessionStore trait 对接外部存储
struct PostgresSessionStore {
    pool: PgPool,
}

#[async_trait]
impl ISessionStore for PostgresSessionStore {
    // 实现持久化逻辑
}
```

## 上下文压缩策略

### 压缩触发时机

在 `AgentBuilder` 中配置压缩：

```rust
let agent = AgentBuilder::new("agent")
    .chat_client(client)
    .with_compression(
        CompressionConfig::new()
            .with_strategy(CompressionStrategy::Summarize) // 摘要压缩
            .with_threshold(8000)                          // 超过 8000 tokens 触发
            .with_target_tokens(4000)                      // 压缩到 4000 tokens
    )
    .build()?;
```

### 压缩策略对比

| 策略 | 精度损失 | 速度 | 适用场景 |
|------|---------|------|---------|
| `Truncate` | 高 | 极快 | 简单场景，不关心历史 |
| `Summarize` | 中 | 中等 | 需要保留上下文语义 |
| `SelectiveReduce` | 低 | 慢 | 需要高精度上下文保留 |

### Token 预算管理

```rust
// 设置 token 预算
let options = AgentRunOptions {
    max_tokens: Some(4096),          // 响应 token 上限
    compression_threshold: Some(7000), // 压缩触发阈值
    ..Default::default()
};
```

## 并行工具执行

### 顺序 vs 并行

当 LLM 在一次响应中请求调用多个工具时：

```rust
// 顺序执行（默认）
// Tool1 → Tool2 → Tool3

// 配置并行执行
let agent = AgentBuilder::new("agent")
    .chat_client(client)
    .parallel_tool_execution(true) // 启用并行工具执行
    .max_parallel_tools(5)         // 最多 5 个工具并行
    .build()?;
```

### 性能对比

| 场景 | 顺序执行 | 并行执行 | 加速比 |
|------|---------|---------|--------|
| 3 个独立工具（各 2s） | 6s | 2s | 3x |
| 5 个独立工具（各 1s） | 5s | 1s | 5x |
| 有依赖关系的工具 | N/A（必须顺序） | N/A | 无 |

### 注意事项

- 并行执行仅适用于无数据依赖的工具调用
- 有 `tool_call_id` 依赖链的工具会自动顺序执行
- 设置合理的 `max_parallel_tools` 避免资源耗尽

## max_rounds 调优

### 默认值分析

```rust
// 默认值
const DEFAULT_MAX_TOOL_ROUNDS: usize = 10;

// 不同场景的建议值
let coding_agent = AgentBuilder::new("coder")
    .max_tool_rounds(20)  // 代码生成需要更多工具调用
    .build()?;

let qa_agent = AgentBuilder::new("qa")
    .max_tool_rounds(5)   // 问答通常不需要太多工具
    .build()?;

let research_agent = AgentBuilder::new("researcher")
    .max_tool_rounds(15)  // 搜索研究需要多次网络查询
    .build()?;
```

### 成本控制

| max_rounds | 最大 API 调用次数 | 预估最大延迟 |
|-----------|-----------------|------------|
| 5 | ~6 (1 次初始 + 5 次工具) | ~30s |
| 10 | ~11 | ~60s |
| 20 | ~21 | ~120s |

## Token 计数器精度

### Tiktoken vs 估算

```toml
[dependencies.rust-agent-framework]
features = ["tiktoken"]  # 启用精确 token 计数
```

| 方法 | 精度 | 性能开销 | 说明 |
|------|------|---------|------|
| `tiktoken-rs` (feature) | ~99% | 中等 | 使用 OpenAI tokenizer |
| 字符/4 估算 | ~75% | 极低 | 粗略估算 |

### 建议

- 生产环境启用 `tiktoken` feature 以获得精确计数
- 开发环境可关闭以减少依赖体积
- Token 计数用于压缩触发判断，精度影响压缩时机

## 流式缓冲区大小

### 配置

```rust
// ChatClient 流式缓冲区
let client = DeepSeekChatClient::new(
    ChatClientOptions::deepseek("model", key)
        .with_stream_buffer_size(8192)     // 8KB 缓冲区（默认）
);

// WebSocket 传输缓冲区
// rust-agent-host 中使用 64KB 双工通道
let (dup_a, mut dup_b) = tokio::io::duplex(64 * 1024);
```

### 缓冲区大小影响

| 大小 | 延迟 | 吞吐量 | 适用场景 |
|------|------|-------|---------|
| 4KB | 低 | 中 | 交互式对话 |
| 8KB | 中 | 高 | 通用场景（默认） |
| 64KB | 高 | 极高 | 批量代码生成 |

## Checkpoint 性能优化

### 配置建议

```rust
// 高性能场景
let config = CheckpointConfig {
    full_snapshot_interval: 100,   // 增大全量间隔减少 I/O
    max_checkpoints: 30,           // 减少存储占用
    enabled: true,
};

// 高可用场景
let config = CheckpointConfig {
    full_snapshot_interval: 10,    // 频繁全量快照确保恢复速度
    max_checkpoints: 200,          // 更多历史版本
    enabled: true,
};

// 开发调试（关闭）
let config = CheckpointConfig::disabled();
```

### I/O 分析

| 操作 | 开销 | 频率 |
|------|------|------|
| 增量快照 | ~1-5ms | 每个 SuperStep |
| 全量快照 | ~10-50ms | 每 N 个 SuperStep |
| 恢复加载 | ~50-200ms | 启动时 |

## 内存优化

### Agent 池化

```rust
// 重用 Agent 实例（Agent 是无状态的）
let shared_agent = Arc::new(
    AgentBuilder::new("shared")
        .chat_client(client.clone())
        .build()?
);

// 多个 session 共享同一个 Agent
registry.register(shared_agent);
```

### 会话过期

```rust
// 设置会话 TTL
let config = HostConfig {
    session_ttl_secs: Some(3600), // 1 小时过期
    max_sessions: Some(1000),      // 最大会话数
    ..Default::default()
};
```

## 性能基准参考

| 操作 | 典型延迟 | 说明 |
|------|---------|------|
| Agent 初始化 | ~1ms | 构建 Agent 实例 |
| 首次 LLM 调用 | ~100-2000ms | 取决于模型和网络 |
| 工具执行（本地） | ~1-10ms | 文件系统操作 |
| 工具执行（网络） | ~100-2000ms | WebSearch/WebFetch |
| 会话消息追加 | ~1μs | 内存存储 |
| 检查点保存（增量） | ~1-5ms | 文件存储 |
| 上下文压缩 | ~500-2000ms | 需要 LLM 调用 |
| 流式 chunk 间隔 | ~20-50ms | 取决于模型 |

## 生产环境清单

1. ✅ 使用外部会话存储（PostgreSQL/Redis）
2. ✅ 启用 `tiktoken` feature 精确 token 计数
3. ✅ 配置合理的上下文压缩策略
4. ✅ 设置 `max_tool_rounds` 防止无限循环
5. ✅ 启用检查点（生产环境）
6. ✅ 配置并行工具执行（独立工具场景）
7. ✅ 使用 WebSocket 传输模式（服务化部署）
8. ✅ 设置会话 TTL 防止内存泄漏
9. ✅ 监控 MemoryConsolidationWorker 的 stats
10. ✅ 使用 `SubAgentStatusTracker` 追踪编排性能
