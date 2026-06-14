# 上下文窗口压缩策略实现计划

> **目标**：为 Agent 管道引入可组合的上下文窗口压缩策略，在消息拼装完成、LLM 调用前对消息列表进行变换，确保不超出 token 预算。

---

## 一、设计定位

### 1.1 压缩策略在管道中的位置

`ICompressionStrategy` 是 **独立 trait**，与 `IContextProvider` 平级但职责不同：

- `IContextProvider`：**注入**上下文（instructions / messages / tools）→ 做加法
- `ICompressionStrategy`：**压缩**上下文（截断 / 摘要 / 滑动窗口）→ 做减法

```
ChatClientAgent.run():
  Phase 1:   providers.on_invoking()      → 注入上下文（做加法）
  Phase 1.5: compression_strategy.compress() → 压缩消息列表（做减法）← 新增
  Phase 2:   chat_client.run(messages)     → LLM 调用
  Phase 3:   [channel fork] providers.on_invoked() → 后处理
```

运行时机：**所有 provider 注入完毕、完整消息列表拼装完成后**，LLM 调用前。

### 1.2 为什么不是 IContextProvider 子类型

| 维度 | IContextProvider | ICompressionStrategy |
|------|-----------------|---------------------|
| 管道阶段 | Phase 1（注入） | Phase 1.5（变换） |
| 输入 | 调用方原始 messages | 已拼装完整 messages（含 history 等） |
| 输出语义 | **追加**到消息列表 | **替换**消息列表 |
| 组合方式 | 链式累加 | 链式管道（pipe） |
| 执行次数 | 每轮调用 1 次 | 每轮调用 1 次 |

两者语义不兼容——强制合并会引入歧义（"Provider 注入是否替换已有消息？"）。独立 trait 让职责边界清晰。

---

## 二、核心类型设计

### 2.1 `ICompressionStrategy` trait（core crate）

**文件**：`crates/core/src/compression.rs`（**新建**）

```rust
use async_trait::async_trait;
use crate::{ChatMessage, Result};

/// 上下文压缩策略 trait
///
/// 在消息组装完成后、LLM 调用前执行。接收完整消息列表，
/// 返回压缩后的消息列表。system 消息被单独传入以避免被裁剪。
///
/// 策略可组合：`CompressionStrategyBuilder` 将多个策略串联为管道。
#[async_trait]
pub trait ICompressionStrategy: Send + Sync {
    /// 策略名称，用于日志和调试
    fn name(&self) -> &str;

    /// 压缩消息列表
    ///
    /// # 参数
    /// - messages: 已拼装的完整消息列表（含 history + provider 注入 + caller）
    /// - system_message: system 消息（单独传入，保证不被裁剪）
    ///
    /// # 返回
    /// 压缩后的消息列表（不含 system 消息，由调用方重新拼装）
    ///
    /// # 合约
    /// - system 消息不参与压缩、不被裁剪
    /// - 返回的 messages 顺序与原列表一致
    async fn compress(
        &self,
        messages: &[ChatMessage],
        system_message: Option<&ChatMessage>,
    ) -> Result<Vec<ChatMessage>>;
}
```

**设计决策**：
- `system_message` 单独传入：保证 system prompt 永远不被压缩截断
- 返回 `Vec<ChatMessage>` 不含 system：调用方负责 `[system] + compressed_messages`，语义清晰
- 压缩参数（max_tokens, max_messages）由各策略自身持有，不通过 trait 传递——每个策略自描述

### 2.2 `TokenCounter` 工具类型（framework crate）

**文件**：`crates/framework/src/compression/token_counter.rs`（**新建**）

```rust
/// 简单 token 估算器（基于字符数比例）
/// 生产环境可替换为 tiktoken-rs 等精确计数
pub struct TokenCounter {
    /// 平均 token 与字符的比例（英文约 4 chars/token，中文约 1.5 chars/token）
    chars_per_token: f32,
}

impl TokenCounter {
    pub fn new() -> Self { Self { chars_per_token: 4.0 } }
    
    /// Estimate token count for a message
    pub fn count_message(&self, msg: &ChatMessage) -> usize {
        (msg.content.chars().count() as f32 / self.chars_per_token).ceil() as usize
    }
    
    /// Estimate total token count for message list
    pub fn count_messages(&self, messages: &[ChatMessage]) -> usize {
        messages.iter().map(|m| self.count_message(m)).sum()
    }
}
```

---

## 三、内置策略实现

所有内置策略放在 `crates/framework/src/compression/` 目录下。

### 3.1 `SlidingWindowStrategy` — 滑动窗口

**文件**：`crates/framework/src/compression/sliding_window.rs`（**新建**）

保留最近 N 条消息，丢弃更早的消息。

```rust
pub struct SlidingWindowStrategy {
    /// 保留最近 N 条消息（不含 system）
    max_messages: usize,
}

impl SlidingWindowStrategy {
    pub fn new(max_messages: usize) -> Self { Self { max_messages } }
}

#[async_trait]
impl ICompressionStrategy for SlidingWindowStrategy {
    fn name(&self) -> &str { "SlidingWindow" }

    async fn compress(
        &self,
        messages: &[ChatMessage],
        _system_message: Option<&ChatMessage>,
    ) -> Result<Vec<ChatMessage>> {
        if messages.len() <= self.max_messages {
            return Ok(messages.to_vec());
        }
        let start = messages.len() - self.max_messages;
        Ok(messages[start..].to_vec())
    }
}
```

### 3.2 `TokenBudgetStrategy` — Token 预算截断

**文件**：`crates/framework/src/compression/token_budget.rs`（**新建**）

从头部开始丢弃消息，直到总 token 数在预算内。保留最近的消息优先。

```rust
pub struct TokenBudgetStrategy {
    max_tokens: usize,
    counter: TokenCounter,
}

impl TokenBudgetStrategy {
    pub fn new(max_tokens: usize) -> Self {
        Self { max_tokens, counter: TokenCounter::new() }
    }
}

#[async_trait]
impl ICompressionStrategy for TokenBudgetStrategy {
    fn name(&self) -> &str { "TokenBudget" }

    async fn compress(
        &self,
        messages: &[ChatMessage],
        _system_message: Option<&ChatMessage>,
    ) -> Result<Vec<ChatMessage>> {
        let total = self.counter.count_messages(messages);
        if total <= self.max_tokens {
            return Ok(messages.to_vec());
        }
        // 从头部开始丢弃，保留尾部（最近的消息）
        let mut keep = Vec::new();
        let mut running = 0usize;
        for msg in messages.iter().rev() {
            let cost = self.counter.count_message(msg);
            if running + cost > self.max_tokens {
                break;
            }
            running += cost;
            keep.push(msg.clone());
        }
        keep.reverse();
        Ok(keep)
    }
}
```

### 3.3 `SummarizationStrategy` — LLM 摘要压缩

**文件**：`crates/framework/src/compression/summarization.rs`（**新建**）

使用一个独立的 `IChatClient` 调用 LLM 将旧消息摘要为一句话，保留最近的消息不变。

```rust
pub struct SummarizationStrategy {
    /// 用于生成摘要的 LLM client（可以是一个廉价模型）
    chat_client: Arc<dyn IChatClient>,
    /// 保留最近 N 条消息不做摘要
    keep_recent: usize,
    /// 摘要的最大 token 数（估算）
    max_summary_tokens: usize,
    /// 超过此数量才触发摘要
    trigger_message_count: usize,
}

impl SummarizationStrategy {
    pub fn new(chat_client: Arc<dyn IChatClient>) -> Self {
        Self {
            chat_client,
            keep_recent: 10,
            max_summary_tokens: 500,
            trigger_message_count: 20,
        }
    }
}

#[async_trait]
impl ICompressionStrategy for SummarizationStrategy {
    fn name(&self) -> &str { "Summarization" }

    async fn compress(
        &self,
        messages: &[ChatMessage],
        _system_message: Option<&ChatMessage>,
    ) -> Result<Vec<ChatMessage>> {
        if messages.len() <= self.trigger_message_count {
            return Ok(messages.to_vec());
        }

        // 新旧分界：保留最近 keep_recent 条
        let split = messages.len().saturating_sub(self.keep_recent);
        let old = &messages[..split];
        let recent = &messages[split..];

        // 构建摘要请求
        let conversation: String = old
            .iter()
            .map(|m| format!("[{}]: {}", 
                match m.role { crate::MessageRole::User => "用户", _ => "助手" },
                m.content))
            .collect::<Vec<_>>()
            .join("\n");

        let summary_prompt = ChatMessage::user(format!(
            "请用不超过 {} 字的中文一句话总结以下对话的核心内容和关键决策：\n\n{}",
            self.max_summary_tokens * 4, conversation
        ));

        // 调用 LLM 生成摘要
        let summary = match self.chat_client.run(&[summary_prompt], ChatClientRunOptions::default()).await {
            Ok(mut stream) => {
                // 收集流（简单场景，不关心流式）
                // 注：此处需要实现收集逻辑或使用已有的 IChatClient 非流式 API
                let mut text = String::new();
                use futures_util::StreamExt;
                while let Some(Ok(update)) = stream.next().await {
                    if let crate::AgentResponseUpdate::TextDelta { delta } = update {
                        text.push_str(&delta);
                    }
                }
                text
            }
            Err(_) => "[摘要生成失败]".to_string(),
        };

        // 组装结果：[摘要消息] + [最近消息]
        let mut result = vec![ChatMessage::user(format!("[对话历史摘要]\n{}", summary))];
        result.extend_from_slice(recent);
        Ok(result)
    }
}
```

---

## 四、策略组合：`CompressionStrategyBuilder`

**文件**：`crates/framework/src/compression/builder.rs`（**新建**）

```rust
use std::sync::Arc;
use rust_agent_core::{ChatMessage, ICompressionStrategy, Result};

/// 组合多个压缩策略为链式管道
///
/// 策略按注册顺序执行，前一个的输出是后一个的输入。
///
/// ```ignore
/// let strategy = CompressionStrategyBuilder::new()
///     .add(SlidingWindowStrategy::new(50))
///     .add(TokenBudgetStrategy::new(8000))
///     .build();
/// // 先截断到 50 条，再按 token 预算截断
/// ```
pub struct CompressionStrategyBuilder {
    strategies: Vec<Arc<dyn ICompressionStrategy>>,
}

impl CompressionStrategyBuilder {
    pub fn new() -> Self {
        Self { strategies: Vec::new() }
    }

    /// 添加一个压缩策略
    pub fn add(mut self, strategy: impl ICompressionStrategy + 'static) -> Self {
        self.strategies.push(Arc::new(strategy));
        self
    }

    /// 构建组合策略
    pub fn build(self) -> CompositeCompressionStrategy {
        CompositeCompressionStrategy {
            strategies: self.strategies,
        }
    }
}

impl Default for CompressionStrategyBuilder {
    fn default() -> Self { Self::new() }
}

/// 组合压缩策略——链式管道执行
pub struct CompositeCompressionStrategy {
    strategies: Vec<Arc<dyn ICompressionStrategy>>,
}

#[async_trait]
impl ICompressionStrategy for CompositeCompressionStrategy {
    fn name(&self) -> &str { "CompositeCompression" }

    async fn compress(
        &self,
        messages: &[ChatMessage],
        system_message: Option<&ChatMessage>,
    ) -> Result<Vec<ChatMessage>> {
        let mut result = messages.to_vec();
        for strategy in &self.strategies {
            result = strategy.compress(&result, system_message).await?;
        }
        Ok(result)
    }
}
```

---

## 五、管道集成

### 5.1 ChatClientAgent 新增压缩步骤

**文件**：`crates/framework/src/chat_client_agent.rs`（**修改**）

在结构体中新增字段并在 `run()` 中插入 Phase 1.5：

```rust
pub struct ChatClientAgent {
    // ... 现有字段
    compression_strategy: Option<Arc<dyn ICompressionStrategy>>,  // 新增
}

// 在 run() 中，KV cache 追踪之前插入：
// ── Phase 1.5: Compression ────────────────────────────────────
if let Some(ref strategy) = self.compression_strategy {
    let system_msg = full_messages.iter().find(|m| m.role == MessageRole::System);
    let non_system: Vec<ChatMessage> = full_messages
        .iter()
        .filter(|m| m.role != MessageRole::System)
        .cloned()
        .collect();
    
    let compressed = strategy
        .compress(&non_system, system_msg)
        .await
        .unwrap_or(non_system);
    
    // 重新拼装：[system] + [compressed]
    full_messages = if let Some(sys) = system_msg.cloned() {
        let mut v = vec![sys];
        v.extend(compressed);
        v
    } else {
        compressed
    };
}
```

### 5.2 AgentBuilder 新增方法

**文件**：`crates/framework/src/builder.rs`（**修改**）

```rust
// 新增字段
compression_strategy: Option<Arc<dyn ICompressionStrategy>>,

// 新增方法
/// 设置上下文压缩策略。
///
/// 可使用 `CompressionStrategyBuilder` 组合多个策略：
///
/// ```ignore
/// let agent = AgentBuilder::new("agent")
///     .chat_client(client)
///     .with_compression_strategy(
///         CompressionStrategyBuilder::new()
///             .add(SlidingWindowStrategy::new(50))
///             .add(TokenBudgetStrategy::new(8000))
///             .build()
///     )
///     .build()?;
/// ```
pub fn with_compression_strategy(
    mut self,
    strategy: impl ICompressionStrategy + 'static,
) -> Self {
    self.compression_strategy = Some(Arc::new(strategy));
    self
}

// build() 中注入：
agent = agent.with_compression_strategy(self.compression_strategy);
```

---

## 六、core crate 导出

**文件**：`crates/core/src/lib.rs`（**修改**）

```rust
pub mod compression;  // 新增

pub use compression::ICompressionStrategy;  // 新增
```

**文件**：`crates/core/src/compression.rs`（**新建**）— 仅包含 trait 定义

---

## 七、framework crate 导出

**文件**：`crates/framework/src/lib.rs`（**修改**）

```rust
pub mod compression;  // 新增

pub use compression::builder::CompressionStrategyBuilder;
pub use compression::sliding_window::SlidingWindowStrategy;
pub use compression::token_budget::TokenBudgetStrategy;
pub use compression::summarization::SummarizationStrategy;
```

---

## 八、文件变更汇总

| 操作 | 文件 | 说明 |
|------|------|------|
| **新建** | `crates/core/src/compression.rs` | `ICompressionStrategy` trait 定义 |
| **新建** | `crates/framework/src/compression/mod.rs` | 模块入口 |
| **新建** | `crates/framework/src/compression/token_counter.rs` | `TokenCounter` 估算工具 |
| **新建** | `crates/framework/src/compression/sliding_window.rs` | `SlidingWindowStrategy` |
| **新建** | `crates/framework/src/compression/token_budget.rs` | `TokenBudgetStrategy` |
| **新建** | `crates/framework/src/compression/summarization.rs` | `SummarizationStrategy` |
| **新建** | `crates/framework/src/compression/builder.rs` | `CompressionStrategyBuilder` + `CompositeCompressionStrategy` |
| **修改** | `crates/core/src/lib.rs` | 导出 `compression` 模块 + `ICompressionStrategy` |
| **修改** | `crates/framework/src/chat_client_agent.rs` | 新增 `compression_strategy` 字段 + Phase 1.5 管道步骤 |
| **修改** | `crates/framework/src/builder.rs` | 新增 `with_compression_strategy()` 方法 |
| **修改** | `crates/framework/src/lib.rs` | 导出 compression 模块和内置策略 |

---

## 九、使用示例

```rust
use rust_agent_framework::compression::{
    CompressionStrategyBuilder, SlidingWindowStrategy, TokenBudgetStrategy,
};

// ── 单策略：滑动窗口 ──
let agent = AgentBuilder::new("agent")
    .chat_client(client)
    .with_compression_strategy(SlidingWindowStrategy::new(20))
    .build()?;

// ── 组合策略：滑动窗口 + Token 预算 ──
let agent = AgentBuilder::new("agent")
    .chat_client(client)
    .with_compression_strategy(
        CompressionStrategyBuilder::new()
            .add(SlidingWindowStrategy::new(50))
            .add(TokenBudgetStrategy::new(8000))
            .build()
    )
    .build()?;

// ── 完整组合：摘要 + 窗口 + 预算 ──
let agent = AgentBuilder::new("agent")
    .chat_client(client)
    .add_context_provider(SkillsProvider::new())
    .with_compression_strategy(
        CompressionStrategyBuilder::new()
            .add(SummarizationStrategy::new(summary_client))  // 旧消息摘要
            .add(SlidingWindowStrategy::new(30))              // 最多 30 条
            .add(TokenBudgetStrategy::new(8000))              // 不超预算
            .build()
    )
    .build()?;
```

---

## 十、架构决策记录 (ADR)

### ADR-008: ICompressionStrategy 作为独立 trait

**决策**：`ICompressionStrategy` 不与 `IContextProvider` 继承，作为独立 trait。

**理由**：
1. 语义不兼容：Provider 是"注入"，Compression 是"变换"
2. 管道位置不同：Provider 在 Phase 1，Compression 在 Phase 1.5
3. 数据流不同：Provider 输出追加到列表，Compression 输出替换列表
4. 组合方式不同：Provider 是累加链，Compression 是管道链

### ADR-009: system 消息独立传入

**决策**：`compress()` 的 `system_message` 参数单独传入，不参与压缩。

**理由**：
1. System prompt 是 Agent 的核心行为定义，裁剪会导致行为偏差
2. System prompt 通常较短，不需要压缩
3. 调用方（ChatClientAgent）负责最终的 `[system] + compressed` 拼装

### ADR-010: 组合策略采用链式管道

**决策**：`CompositeCompressionStrategy` 按注册顺序依次执行。

**理由**：
1. 顺序管道语义清晰——前一个输出是后一个输入
2. 典型顺序：Summarization → SlidingWindow → TokenBudget
3. 每个策略可以假设输入已被前面的策略粗加工过

### ADR-011: Token 计数采用字符比例估算

**决策**：Phase 1 使用简单字符比例估算。

**理由**：
1. 避免引入 `tiktoken-rs` 等重依赖
2. 估算精度对截断策略足够（保持在预算的 ±20%）
3. TokenCounter 为 trait，未来可替换为精确实现

---

## 十一、验证方案

### 编译 + 测试

```bash
cargo build --workspace && cargo test --workspace && cargo clippy --workspace
```

### 功能验证

| 场景 | 验证点 |
|------|--------|
| 无压缩策略 | 管道行为不变，无 Phase 1.5 开销 |
| 滑动窗口 | 消息数超过 max_messages 时截断 |
| Token 预算 | 估算 token 超过预算时截断 |
| 摘要策略 | 旧消息被摘要，最近消息保留 |
| 组合策略 | 顺序执行正确 |
| system 保护 | system 消息始终保留 |
