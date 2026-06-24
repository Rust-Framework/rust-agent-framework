//! Retrieval trait layer — pluggable backends for future semantic search over bundle concepts.
//!
//! Pluggable retrieval backends (`IMemoryStore`, `IEmbeddingModel`) for semantic search.

use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;

use rust_agent_core::Result;

/// 记忆条目——记忆存储的最小单元
#[derive(Debug, Clone)]
pub struct MemoryEntry {
    /// 条目 ID（唯一标识）
    pub id: String,
    /// 条目内容
    pub content: String,
    /// 条目类型（如 "preference"、"rule"、"lesson"）
    pub kind: String,
    /// 语义标签（用于过滤）
    pub tags: Vec<String>,
    /// 最后更新时间戳（Unix 秒）
    pub updated_at: i64,
}

/// 记忆存储抽象
///
/// 实现者决定如何持久化和检索记忆条目。
///
/// ## 与 MAF 的对照
///
/// | MAF | RAF |
/// |-----|-----|
/// | `VolatileMemoryStore` | `VolatileMemoryStore`（会话临时） |
/// | `SemanticTextMemory` | `VectorMemoryStore`（未来，依赖 `IEmbeddingModel`） |
/// | Connector 生态 | `FileMemoryStore`（当前默认）、`SqliteMemoryStore`（未来） |
#[async_trait]
pub trait IMemoryStore: Send + Sync {
    /// 存储名称（用于诊断）
    fn name(&self) -> &str;

    /// 保存一条记忆
    ///
    /// 如果 `entry.id` 已存在，则更新（增量更新）。
    async fn save(&self, entry: MemoryEntry) -> Result<()>;

    /// 按 ID 读取一条记忆
    async fn load(&self, id: &str) -> Result<Option<MemoryEntry>>;

    /// 列出所有记忆条目
    async fn list(&self) -> Result<Vec<MemoryEntry>>;

    /// 按 kind 过滤列出
    async fn list_by_kind(&self, kind: &str) -> Result<Vec<MemoryEntry>> {
        let all = self.list().await?;
        Ok(all.into_iter().filter(|e| e.kind == kind).collect())
    }

    /// 删除一条记忆
    async fn delete(&self, id: &str) -> Result<()>;

    /// 合并记忆——增量更新
    ///
    /// 对标 MAF 的"仅实质性差异触发写入"。
    /// 默认实现比较语义相似度，仅更新变化条目。
    async fn consolidate(&self, entries: Vec<MemoryEntry>) -> Result<ConsolidationReport> {
        let mut report = ConsolidationReport::default();
        for entry in entries {
            let existing = self.load(&entry.id).await?;
            let should_update = match &existing {
                None => true,
                Some(old) => !is_semantically_equal(&old.content, &entry.content),
            };
            if should_update {
                self.save(entry.clone()).await?;
                report.updated.push(entry.id.clone());
            } else {
                report.unchanged.push(entry.id);
            }
        }
        Ok(report)
    }
}

/// 合并报告——记录哪些条目被更新、哪些保持不变
#[derive(Debug, Default, Clone)]
pub struct ConsolidationReport {
    pub updated: Vec<String>,
    pub unchanged: Vec<String>,
}

/// 嵌入模型抽象
///
/// 对标 MAF 的 `ITextEmbeddingGenerationService`。
/// 将文本映射为向量，用于语义相似度计算和向量检索。
///
/// ## 未来实现
///
/// - `OpenAIEmbeddingModel`：使用 OpenAI text-embedding-3-small
/// - `LocalEmbeddingModel`：使用本地模型（如 fastembed-rs）
#[async_trait]
pub trait IEmbeddingModel: Send + Sync {
    /// 模型名称
    fn name(&self) -> &str;

    /// 生成文本的嵌入向量
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;

    /// 批量生成嵌入向量
    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            results.push(self.embed(text).await?);
        }
        Ok(results)
    }

    /// 向量维度
    fn dimension(&self) -> usize;
}

/// 语义相等性检测——判断两段文本是否语义相同
///
/// 当前实现为精确字符串比较（保守策略，避免误判）。
/// 未来可替换为嵌入向量余弦相似度比较。
fn is_semantically_equal(a: &str, b: &str) -> bool {
    a.trim() == b.trim()
}

/// 文件系统检索后端——与 bundle 目录 layout 兼容。
pub struct FileMemoryStore {
    root: PathBuf,
}

impl FileMemoryStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &PathBuf {
        &self.root
    }
}

#[async_trait]
impl IMemoryStore for FileMemoryStore {
    fn name(&self) -> &str {
        "FileMemoryStore"
    }

    async fn save(&self, entry: MemoryEntry) -> Result<()> {
        let path = self.root.join(format!("{}.md", entry.kind));
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                rust_agent_core::AgentError::ConfigError(format!(
                    "Failed to create memory dir: {}",
                    e
                ))
            })?;
        }
        tokio::fs::write(&path, &entry.content).await.map_err(|e| {
            rust_agent_core::AgentError::ConfigError(format!("Failed to write memory: {}", e))
        })?;
        Ok(())
    }

    async fn load(&self, id: &str) -> Result<Option<MemoryEntry>> {
        let path = self.root.join(format!("{}.md", id));
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => Ok(Some(MemoryEntry {
                id: id.to_string(),
                content,
                kind: id.to_string(),
                tags: vec![],
                updated_at: 0,
            })),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(rust_agent_core::AgentError::ConfigError(format!(
                "Failed to read memory: {}",
                e
            ))),
        }
    }

    async fn list(&self) -> Result<Vec<MemoryEntry>> {
        let mut entries = Vec::new();
        let mut rd = match tokio::fs::read_dir(&self.root).await {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => {
                return Err(rust_agent_core::AgentError::ConfigError(format!(
                    "Failed to read memory dir: {}",
                    e
                )))
            }
        };
        while let Some(entry) = rd.next_entry().await.map_err(|e| {
            rust_agent_core::AgentError::ConfigError(format!("Failed to read dir entry: {}", e))
        })? {
            if entry.path().extension().and_then(|s| s.to_str()) == Some("md") {
                if let Some(stem) = entry.path().file_stem().and_then(|s| s.to_str()) {
                    if let Ok(content) = tokio::fs::read_to_string(entry.path()).await {
                        entries.push(MemoryEntry {
                            id: stem.to_string(),
                            content,
                            kind: stem.to_string(),
                            tags: vec![],
                            updated_at: 0,
                        });
                    }
                }
            }
        }
        Ok(entries)
    }

    async fn delete(&self, id: &str) -> Result<()> {
        let path = self.root.join(format!("{}.md", id));
        match tokio::fs::remove_file(&path).await {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(rust_agent_core::AgentError::ConfigError(format!(
                "Failed to delete memory: {}",
                e
            ))),
        }
    }
}

/// 向量记忆存储——未来实现（依赖 IEmbeddingModel）
///
/// 仅检索相关片段而非全量读取，将 Token 消耗从 O(n) 降为 O(k)。
/// 当前为占位，待 IEmbeddingModel 实现就绪后启用。
pub struct VectorMemoryStore {
    #[allow(dead_code)]
    store: Arc<dyn IMemoryStore>,
    #[allow(dead_code)]
    embedder: Arc<dyn IEmbeddingModel>,
}

impl VectorMemoryStore {
    pub fn new(store: Arc<dyn IMemoryStore>, embedder: Arc<dyn IEmbeddingModel>) -> Self {
        Self { store, embedder }
    }
}
