use async_trait::async_trait;
use std::sync::Arc;

use crate::{ISession, Result};

/// 会话持久化存储接口。
///
/// 抽象会话数据的存储后端，支持跨请求和跨重启的会话恢复。
///
/// 参考自 MAF 的 `AgentSessionStore` 设计。
#[async_trait]
pub trait ISessionStore: Send + Sync {
    /// 将会话保存到存储中。
    ///
    /// 如果已存在相同 ID 的会话，则会被覆盖。
    async fn save_session(&self, session: &dyn ISession) -> Result<()>;

    /// 根据 ID 获取会话。
    ///
    /// 如果指定 ID 的会话不存在，返回 `None`。
    async fn get_session(&self, session_id: &str) -> Result<Option<Arc<dyn ISession>>>;

    /// 根据 ID 删除会话。
    ///
    /// 如果会话不存在，不会引发错误。
    async fn delete_session(&self, session_id: &str) -> Result<()>;

    /// 清理过期的会话。
    ///
    /// 返回已移除的会话数量。
    /// 实现应检查 `ISession::last_active_at()` 与配置的 TTL 选项。
    async fn cleanup_expired(&self) -> Result<usize>;
}
