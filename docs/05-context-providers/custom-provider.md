# 5.4 自定义上下文提供器

自定义 `IContextProvider` 是 RAF 推荐的扩展 Agent 行为能力的方式。本章通过两个完整示例展示构建自定义提供器的步骤和最佳实践。

## 基础模板

```rust
use async_trait::async_trait;
use rust_agent_core::{
    ContextInjection, IAgent, IContextProvider, ISession, ChatMessage,
    AgentRunOptions, AgentResponse, AgentError, Result,
};

pub struct MyContextProvider {
    // 提供器的配置和状态
    config: MyConfig,
}

#[async_trait]
impl IContextProvider for MyContextProvider {
    fn name(&self) -> &str {
        "MyContextProvider"
    }

    async fn on_invoking(
        &self,
        _agent: &dyn IAgent,
        _session: &dyn ISession,
        _messages: &[ChatMessage],
        _options: &AgentRunOptions,
    ) -> Result<ContextInjection> {
        // 注入指令、消息或工具
        Ok(ContextInjection::default())
    }

    async fn on_invoked(
        &self,
        _agent: &dyn IAgent,
        _session: &dyn ISession,
        _request_messages: &[ChatMessage],
        _response: Option<&AgentResponse>,
        _error: Option<&AgentError>,
    ) -> Result<()> {
        // 后置处理：持久化、日志等
        Ok(())
    }
}
```

## 示例一：用户画像上下文注入

这个提供器在每个 Agent 调用前查询用户画像，将用户偏好注入 system prompt。

```rust
use async_trait::async_trait;
use rust_agent_core::{
    AgentResponse, AgentRunOptions, ChatMessage, ContextInjection,
    IAgent, IContextProvider, ISession, ProviderState, Result,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// 用户画像数据结构
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct UserProfile {
    name: String,
    preferred_language: String,
    expertise_level: String,
    timezone: String,
}

/// 将用户画像注入 Agent 上下文的提供器
pub struct UserProfileProvider {
    /// 用户 ID（可从请求上下文、JWT token 等获取）
    user_id: String,
    /// 异步获取画像的回调
    profile_fetcher: Arc<dyn Fn(&str) -> Result<UserProfile> + Send + Sync>,
}

impl UserProfileProvider {
    pub fn new(
        user_id: impl Into<String>,
        fetcher: Arc<dyn Fn(&str) -> Result<UserProfile> + Send + Sync>,
    ) -> Self {
        Self {
            user_id: user_id.into(),
            profile_fetcher: fetcher,
        }
    }

    fn build_instructions(&self, profile: &UserProfile) -> String {
        format!(
            "## 当前用户信息\n\
             - 姓名: {name}\n\
             - 偏好语言: {lang}\n\
             - 技能水平: {level}\n\
             - 时区: {tz}\n\n\
             请根据用户的技能水平调整回答的详细程度，\
             使用 {lang} 回复，并注意时区 {tz} 的时间相关计算。",
            name = profile.name,
            lang = profile.preferred_language,
            level = profile.expertise_level,
            tz = profile.timezone,
        )
    }
}

#[async_trait]
impl IContextProvider for UserProfileProvider {
    fn name(&self) -> &str {
        "UserProfileProvider"
    }

    async fn on_invoking(
        &self,
        _agent: &dyn IAgent,
        session: &dyn ISession,
        _messages: &[ChatMessage],
        _options: &AgentRunOptions,
    ) -> Result<ContextInjection> {
        // 获取或从 session 缓存中加载用户画像
        let profile = {
            let state = ProviderState::<UserProfile>::new("UserProfileProvider");
            let mut cached = state.get_or_init(session);

            // 如果缓存为空或用户 ID 变化，重新获取
            if cached.name.is_empty() || cached.name != self.user_id {
                match (self.profile_fetcher)(&self.user_id) {
                    Ok(fresh) => {
                        cached = fresh;
                        // 缓存到 session，下次不需要重新查询
                        let _ = state.save(session, &cached);
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, user_id = %self.user_id,
                            "Failed to fetch user profile, using defaults");
                        // 使用默认画像，不阻断 Agent 调用
                        cached = UserProfile::default();
                    }
                }
            }
            cached
        };

        Ok(ContextInjection {
            instructions: Some(self.build_instructions(&profile)),
            ..Default::default()
        })
    }

    async fn on_invoked(
        &self,
        _agent: &dyn IAgent,
        _session: &dyn ISession,
        _request_messages: &[ChatMessage],
        _response: Option<&AgentResponse>,
        _error: Option<&AgentError>,
    ) -> Result<()> {
        // 可以记录每次调用的用户行为日志
        tracing::info!(
            user_id = %self.user_id,
            has_error = _error.is_some(),
            "Agent invoked for user"
        );
        Ok(())
    }
}
```

### 使用示例

```rust
use std::sync::Arc;

// 模拟从数据库加载用户画像
let profile_fetcher = Arc::new(|user_id: &str| -> Result<UserProfile> {
    // 实际场景中，这里可能是数据库查询或 API 调用
    Ok(UserProfile {
        name: "张三".into(),
        preferred_language: "中文".into(),
        expertise_level: "高级".into(),
        timezone: "Asia/Shanghai".into(),
    })
});

let provider = UserProfileProvider::new("user-001", profile_fetcher);

let agent = AgentBuilder::new()
    .with_context_provider(Arc::new(provider))
    .build()?;
```

## 示例二：数据库查询结果注入

这个提供器在每次 Agent 调用前执行预定义的数据库查询，将查询结果作为上下文注入。

```rust
use async_trait::async_trait;
use rust_agent_core::{
    AgentResponse, AgentRunOptions, ChatMessage, ContextInjection,
    IAgent, IContextProvider, ISession, Result,
};
use std::sync::Arc;

/// 预定义的数据库查询提供器
pub struct DatabaseContextProvider {
    /// 数据库连接池
    pool: sqlx::PgPool,
    /// 预定义的查询列表（名称 → SQL）
    queries: Vec<(String, String)>,
}

impl DatabaseContextProvider {
    pub fn new(pool: sqlx::PgPool, queries: Vec<(String, String)>) -> Self {
        Self { pool, queries }
    }

    async fn execute_queries(&self) -> Result<String> {
        let mut context = String::from("## 系统数据快照\n\n");

        for (name, sql) in &self.queries {
            // 执行查询
            let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(sql)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| {
                    rust_agent_core::AgentError::ConfigError(format!(
                        "查询 '{}' 执行失败: {}", name, e
                    ))
                })?;

            // 格式化结果（简化示例；实际可用 serde 序列化）
            context.push_str(&format!("### {}\n", name));
            if rows.is_empty() {
                context.push_str("无数据\n\n");
            } else {
                context.push_str(&format!("{} 条记录\n\n", rows.len()));
            }
        }

        Ok(context)
    }
}

#[async_trait]
impl IContextProvider for DatabaseContextProvider {
    fn name(&self) -> &str {
        "DatabaseContextProvider"
    }

    async fn on_invoking(
        &self,
        _agent: &dyn IAgent,
        _session: &dyn ISession,
        _messages: &[ChatMessage],
        _options: &AgentRunOptions,
    ) -> Result<ContextInjection> {
        let instructions = match self.execute_queries().await {
            Ok(ctx) => Some(ctx),
            Err(e) => {
                tracing::error!(error = %e, "Failed to execute DB queries");
                None
            }
        };

        Ok(ContextInjection {
            instructions,
            ..Default::default()
        })
    }

    async fn on_invoked(
        &self,
        _agent: &dyn IAgent,
        _session: &dyn ISession,
        _request_messages: &[ChatMessage],
        _response: Option<&AgentResponse>,
        _error: Option<&AgentError>,
    ) -> Result<()> {
        Ok(())
    }
}
```

### 使用示例

```rust
let queries = vec![
    ("活跃用户数".into(), "SELECT COUNT(*) FROM users WHERE last_active > NOW() - INTERVAL '1 day'".into()),
    ("待处理订单".into(), "SELECT COUNT(*) FROM orders WHERE status = 'pending'".into()),
    ("系统健康状态".into(), "SELECT status FROM system_health ORDER BY checked_at DESC LIMIT 1".into()),
];

let provider = DatabaseContextProvider::new(pg_pool, queries);

let agent = AgentBuilder::new()
    .with_context_provider(Arc::new(provider))
    .build()?;
```

## 示例三：消息计数限制提供器

确保 LLM 接收的消息不超过指定数量，超出时自动压缩：

```rust
use async_trait::async_trait;
use rust_agent_core::{
    AgentResponse, AgentRunOptions, ChatMessage, ContextInjection,
    IAgent, IContextProvider, ISession, Result,
};

/// 限制注入消息数量的提供器
pub struct MessageLimitProvider {
    /// 最大消息数（超过后使用 replace_messages）
    max_messages: usize,
    /// 保留最近的消息数
    keep_recent: usize,
}

impl MessageLimitProvider {
    pub fn new(max_messages: usize, keep_recent: usize) -> Self {
        Self { max_messages, keep_recent }
    }
}

#[async_trait]
impl IContextProvider for MessageLimitProvider {
    fn name(&self) -> &str {
        "MessageLimitProvider"
    }

    async fn on_invoking(
        &self,
        _agent: &dyn IAgent,
        _session: &dyn ISession,
        messages: &[ChatMessage],
        _options: &AgentRunOptions,
    ) -> Result<ContextInjection> {
        // 消息数在阈值内 → 不干扰
        if messages.len() <= self.max_messages {
            return Ok(ContextInjection::default());
        }

        // 超出阈值 → 只保留最近 N 条消息
        let start = messages.len().saturating_sub(self.keep_recent);
        let truncated: Vec<ChatMessage> = messages[start..].to_vec();

        Ok(ContextInjection {
            instructions: Some(format!(
                "[上下文压缩] 原始 {} 条消息，截断至最近 {} 条。",
                messages.len(),
                truncated.len()
            )),
            messages: truncated,
            replace_messages: true, // ← 关键：替换前面的消息列表
            ..Default::default()
        })
    }

    async fn on_invoked(
        &self, _agent: &dyn IAgent, _session: &dyn ISession,
        _request_messages: &[ChatMessage],
        _response: Option<&AgentResponse>, _error: Option<&AgentError>,
    ) -> Result<()> {
        Ok(())
    }
}
```

## 提供器排序策略

由于提供器按注册顺序执行，合理安排排序很重要：

```rust
let agent = AgentBuilder::new()
    // 1. 先加载历史（所有消息的基础）
    .with_context_provider(Arc::new(InMemoryHistoryProvider::new()))
    // 2. 注入技能和工具
    .with_context_provider(Arc::new(AgentSkillsProvider::scan("./skills")?))
    // 3. 注入工作区信息
    .with_context_provider(Arc::new(workspace_provider))
    // 4. 注入用户画像（靠后，个性化信息优先显示）
    .with_context_provider(Arc::new(user_profile_provider))
    // 5. 最后压缩（限制消息总数）
    .with_context_provider(Arc::new(MessageLimitProvider::new(50, 20)))
    .build()?;
```

**排序原则：**

1. 数据加载型提供器靠前（历史、状态等）
2. 工具注入型提供器居中（技能、工作区）
3. 个性化/指令型提供器靠后（用户画像、业务规则）
4. 压缩/截断型提供器最后（`replace_messages = true`）

## 最佳实践

1. **错误不应阻断 Agent 调用**——`on_invoking` 失败时返回 `ContextInjection::default()` 比抛出错误更好
2. **使用 `ProviderState` 缓存数据**——避免每次 `on_invoking` 都查询数据库
3. **`on_invoked` 中的 `error` 传参**——可用于失败告警或状态回滚
4. **命名清晰**——`name()` 返回的字符串应能唯一标识提供器
5. **`replace_messages` 谨慎使用**——确保不会误删其他提供器注入的重要消息
