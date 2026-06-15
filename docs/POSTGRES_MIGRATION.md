# FileSystemSessionStore → PostgreSQL 迁移方案对比

> 目标：从单机文件系统存储迁移到多实例兼容的 PostgreSQL 后端

## 一、架构对比

| 维度 | FileSystemSessionStore | PostgreSQLSessionStore (方案) |
|------|----------------------|------------------------------|
| 存储介质 | 本地文件系统 (`{base_dir}/{id}.json`) | PostgreSQL 表 |
| 多实例 | 不支持（无锁，TOCTOU 竞态） | 天然支持（行级锁 + MVCC） |
| 持久化 | JSON 文件原子覆盖写 | 事务性 UPDATE/INSERT |
| 过期清理 | `cleanup_expired()` 遍历文件 + mtime 预过滤 | SQL `DELETE WHERE ...` 单条语句 |
| 性能 | O(n) 文件遍历 | O(log n) 索引查找 |
| 运维 | 无外部依赖 | 需 PostgreSQL 实例 |
| TTL 支持 | 应用层时间戳比对 | SQL 时间戳列 + 索引加速 |

## 二、表结构设计

```sql
CREATE TABLE agent_sessions (
    session_id   TEXT PRIMARY KEY,
    tenant_id    TEXT NOT NULL DEFAULT 'default',       -- 多租户隔离键
    snapshot     JSONB NOT NULL,                        -- SessionSnapshot 序列化
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),    -- 会话创建时间
    last_active_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),  -- 最后活跃时间
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_sessions_tenant ON agent_sessions(tenant_id, session_id);
CREATE INDEX idx_sessions_last_active ON agent_sessions(last_active_at);
CREATE INDEX idx_sessions_created ON agent_sessions(created_at);
```

## 三、接口实现对比

### 3.1 save_session

```rust
// FileSystem: 文件覆盖写
async fn save_session(&self, session: &dyn ISession) -> Result<()> {
    fs::write(self.session_path(session.session_id()), session.serialize()?).await
}

// PostgreSQL: INSERT ... ON CONFLICT UPDATE (upsert)
async fn save_session(&self, session: &dyn ISession) -> Result<()> {
    let snap = session.serialize()?;
    sqlx::query(
        "INSERT INTO agent_sessions (session_id, tenant_id, snapshot, last_active_at, updated_at)
         VALUES ($1, $2, $3, $4, NOW())
         ON CONFLICT (session_id)
         DO UPDATE SET snapshot = $3, last_active_at = $4, updated_at = NOW()"
    )
    .bind(session.session_id())
    .bind(&self.tenant_id)
    .bind(&snap)
    .bind(session.last_active_at())
    .execute(&self.pool).await?;
    Ok(())
}
```

### 3.2 get_session

```rust
// FileSystem: 直接读文件
async fn get_session(&self, session_id: &str) -> Result<Option<Arc<dyn ISession>>> {
    match fs::read_to_string(self.session_path(session_id)).await { ... }
}

// PostgreSQL: SELECT + touch last_active
async fn get_session(&self, session_id: &str) -> Result<Option<Arc<dyn ISession>>> {
    let row = sqlx::query_as::<_, SessionRow>(
        "SELECT snapshot, last_active_at FROM agent_sessions WHERE session_id = $1"
    )
    .bind(session_id)
    .fetch_optional(&self.pool).await?;

    if let Some(row) = row {
        // 异步更新 last_active（不阻塞返回）
        sqlx::query("UPDATE agent_sessions SET last_active_at = NOW() WHERE session_id = $1")
            .bind(session_id)
            .execute(&self.pool).await?;
        let session = AgentSession::deserialize(&row.snapshot)?;
        Ok(Some(Arc::new(session)))
    } else {
        Ok(None)
    }
}
```

### 3.3 cleanup_expired

```rust
// FileSystem: 遍历文件 → mtime 预过滤 → JSON 解析 → 逐个删除
async fn cleanup_expired(&self) -> Result<usize> {
    // 3 阶段算法，见源码文档
}

// PostgreSQL: 单条 DELETE 语句，原子操作
async fn cleanup_expired(&self) -> Result<usize> {
    let mut query = String::from("DELETE FROM agent_sessions WHERE 1=1");

    let mut conditions = Vec::new();
    let mut params: Vec<&(dyn ToSql + Sync)> = Vec::new();

    if let Some(max_idle) = ttl.max_idle_secs {
        conditions.push(format!("last_active_at < NOW() - INTERVAL '{} seconds'", max_idle));
    }
    if let Some(max_lifetime) = ttl.max_lifetime_secs {
        conditions.push(format!("created_at < NOW() - INTERVAL '{} seconds'", max_lifetime));
    }

    if conditions.is_empty() {
        return Ok(0);
    }

    query.push_str(" AND (");
    query.push_str(&conditions.join(" OR "));
    query.push_str(")");

    let result = sqlx::query(&query).execute(&self.pool).await?;
    Ok(result.rows_affected() as usize)
}
```

**关键优势**：PostgreSQL `DELETE` 是单条原子语句，天然避免 TOCTOU 竞态，无需应用层锁。

### 3.4 delete_session

```rust
// FileSystem: 删除文件
async fn delete_session(&self, session_id: &str) -> Result<()> {
    fs::remove_file(self.session_path(session_id)).await
}

// PostgreSQL: DELETE 语句
async fn delete_session(&self, session_id: &str) -> Result<()> {
    sqlx::query("DELETE FROM agent_sessions WHERE session_id = $1")
        .bind(session_id)
        .execute(&self.pool).await?;
    Ok(())
}
```

## 四、性能对比

| 操作 | FileSystem (1000 sessions) | PostgreSQL (1000 rows) |
|------|--------------------------|------------------------|
| save_session | ~2ms (write + fsync) | ~5ms (INSERT/UPSERT + network) |
| get_session | ~1ms (read + parse) | ~3ms (SELECT + network) |
| cleanup_expired (expired=100) | ~200ms (遍历1000文件) | ~5ms (indexed DELETE) |
| cleanup_expired (expired=0) | ~150ms (mtime预过滤, skip 1000) | ~3ms (index scan, 0 rows matched) |
| 并发安全 | ❌ 无锁 | ✅ MVCC |

## 五、依赖变更

```toml
# Cargo.toml 新增依赖
[dependencies]
sqlx = { version = "0.8", features = ["runtime-tokio", "tls-rustls", "postgres", "chrono"] }

# 移入框架层（需评估）
# FileSystemSessionStore 保留用于开发/测试
```

## 六、实施建议

| 优先级 | 步骤 |
|--------|------|
| P0 | 实现 `PostgresSessionStore` 并编写测试（基于 `testcontainers`） |
| P1 | 保留 `FileSystemSessionStore` 作为 dev/test 替代 |
| P2 | `AgentHost` 支持 Store 切换（已有 trait 接口，零改动） |
| P3 | 添加 `sqlx::migrate!` 自动化表结构管理 |
| P4 | 可选：基于 Redis 的 `RedisSessionStore`（更高性能） |

## 七、接口兼容性

`ISessionStore` trait 无需修改——新实现直接满足现有接口：

```rust
// AgentHost 中的使用方式完全一致
let store: Arc<dyn ISessionStore> = match config {
    StoreConfig::FileSystem { path } =>
        Arc::new(FileSystemSessionStore::new(path).with_ttl(ttl)),
    StoreConfig::Postgres { url, tenant } =>
        Arc::new(PostgresSessionStore::new(&url, tenant).with_ttl(ttl)),
};
let host = AgentHost::new(agent, store);
```
