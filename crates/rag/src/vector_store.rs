use serde::{Deserialize, Serialize};

use crate::types::{DocumentChunk, SearchResult, TextChunk};

// ─── 向量存储 error ─────────────────────────────────────

/// 向量存储错误类型
#[derive(Debug, thiserror::Error)]
pub enum VectorStoreError {
    #[error("维度不匹配: 预期 {expected}, 实际 {actual}")]
    DimensionMismatch { expected: usize, actual: usize },

    #[error("未找到索引: {0}")]
    NotFound(String),

    #[error("存储内部错误: {0}")]
    Internal(String),
}

// ─── 距离度量 ───────────────────────────────────────────

/// 向量距离（相似度）度量方式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DistanceMetric {
    /// 余弦相似度（默认）
    Cosine,
    /// 欧几里得距离（L2）
    Euclidean,
    /// 点积
    DotProduct,
}

// ─── 索引条目 ───────────────────────────────────────────

/// 向量索引中的一条记录
#[derive(Debug, Clone)]
pub struct IndexEntry {
    pub chunk: TextChunk,
    pub embedding: Vec<f32>,
}

// ─── 向量存储 trait ─────────────────────────────────────

/// 向量存储接口
pub trait IVectorStore: Send + Sync {
    /// 存储名称
    fn name(&self) -> &str;

    /// 向量维度
    fn dimensions(&self) -> usize;

    /// 条目数量
    fn count(&self) -> usize;

    /// 添加一个文档块到索引
    fn add(&mut self, chunk: DocumentChunk) -> Result<(), VectorStoreError>;

    /// 批量添加文档块
    fn add_batch(&mut self, chunks: Vec<DocumentChunk>) -> Result<(), VectorStoreError>;

    /// 根据 query 向量检索最相似的 k 个结果
    fn search(
        &self,
        query: &[f32],
        k: usize,
        metric: DistanceMetric,
    ) -> Result<Vec<SearchResult>, VectorStoreError>;

    /// 按 ID 删除条目
    fn delete(&mut self, id: &str) -> Result<(), VectorStoreError>;

    /// 清空存储
    fn clear(&mut self);
}

// ─── VectorStore 枚举 ──────────────────────────────────

/// 向量存储枚举（内置实现 + 扩展点）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VectorStore {
    /// 内存向量存储
    InMemory {
        dimensions: usize,
        metric: DistanceMetric,
    },
}

impl IVectorStore for VectorStore {
    fn name(&self) -> &str {
        match self {
            Self::InMemory { .. } => "in-memory",
        }
    }

    fn dimensions(&self) -> usize {
        match self {
            Self::InMemory { dimensions, .. } => *dimensions,
        }
    }

    fn count(&self) -> usize {
        match self {
            Self::InMemory { dimensions: _, metric: _ } => {
                // 在枚举实现中不跟踪 count，使用 InMemoryVectorStore
                0
            }
        }
    }

    fn add(&mut self, _chunk: DocumentChunk) -> Result<(), VectorStoreError> {
        match self {
            Self::InMemory { .. } => {
                Err(VectorStoreError::Internal("Use InMemoryVectorStore directly for mutation operations".into()))
            }
        }
    }

    fn add_batch(&mut self, _chunks: Vec<DocumentChunk>) -> Result<(), VectorStoreError> {
        match self {
            Self::InMemory { .. } => {
                Err(VectorStoreError::Internal("Use InMemoryVectorStore directly for mutation operations".into()))
            }
        }
    }

    fn search(
        &self,
        _query: &[f32],
        _k: usize,
        _metric: DistanceMetric,
    ) -> Result<Vec<SearchResult>, VectorStoreError> {
        match self {
            Self::InMemory { .. } => {
                Err(VectorStoreError::Internal("Use InMemoryVectorStore directly for search operations".into()))
            }
        }
    }

    fn delete(&mut self, _id: &str) -> Result<(), VectorStoreError> {
        match self {
            Self::InMemory { .. } => {
                Err(VectorStoreError::Internal("Use InMemoryVectorStore directly for mutation operations".into()))
            }
        }
    }

    fn clear(&mut self) {
        // no-op for enum variant
    }
}

// ─── 内存向量存储（具体实现）──────────────────────────

/// 内存向量存储 — 将向量和文档块保存在内存中
#[derive(Debug, Clone)]
pub struct InMemoryVectorStore {
    entries: Vec<IndexEntry>,
    dimensions: usize,
    metric: DistanceMetric,
}

impl InMemoryVectorStore {
    pub fn new(dimensions: usize) -> Self {
        Self {
            entries: Vec::new(),
            dimensions,
            metric: DistanceMetric::Cosine,
        }
    }

    pub fn with_metric(mut self, metric: DistanceMetric) -> Self {
        self.metric = metric;
        self
    }

    /// 获取所有条目（只读）
    pub fn entries(&self) -> &[IndexEntry] {
        &self.entries
    }
}

impl IVectorStore for InMemoryVectorStore {
    fn name(&self) -> &str {
        "in-memory"
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn count(&self) -> usize {
        self.entries.len()
    }

    fn add(&mut self, chunk: DocumentChunk) -> Result<(), VectorStoreError> {
        let embedding = chunk.embedding.ok_or_else(|| {
            VectorStoreError::Internal("DocumentChunk missing embedding".into())
        })?;

        if embedding.vector.len() != self.dimensions {
            return Err(VectorStoreError::DimensionMismatch {
                expected: self.dimensions,
                actual: embedding.vector.len(),
            });
        }

        self.entries.push(IndexEntry {
            chunk: chunk.chunk,
            embedding: embedding.vector,
        });
        Ok(())
    }

    fn add_batch(&mut self, chunks: Vec<DocumentChunk>) -> Result<(), VectorStoreError> {
        for chunk in chunks {
            self.add(chunk)?;
        }
        Ok(())
    }

    fn search(
        &self,
        query: &[f32],
        k: usize,
        metric: DistanceMetric,
    ) -> Result<Vec<SearchResult>, VectorStoreError> {
        if query.len() != self.dimensions {
            return Err(VectorStoreError::DimensionMismatch {
                expected: self.dimensions,
                actual: query.len(),
            });
        }

        let query_embedding = crate::types::Embedding::new(query.to_vec());

        let mut scored: Vec<(usize, f64)> = self
            .entries
            .iter()
            .enumerate()
            .map(|(idx, entry)| {
                let entry_embedding = crate::types::Embedding::new(entry.embedding.clone());
                let score = match metric {
                    DistanceMetric::Cosine => query_embedding.cosine_similarity(&entry_embedding),
                    DistanceMetric::Euclidean => {
                        let dist: f32 = query
                            .iter()
                            .zip(&entry.embedding)
                            .map(|(a, b)| (a - b).powi(2))
                            .sum::<f32>()
                            .sqrt();
                        1.0 / (1.0 + dist as f64) // 转为相似度
                    }
                    DistanceMetric::DotProduct => {
                        let dot: f32 = query.iter().zip(&entry.embedding).map(|(a, b)| a * b).sum();
                        dot as f64
                    }
                };
                (idx, score)
            })
            .collect();

        // 按分数降序排列
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let results: Vec<SearchResult> = scored
            .into_iter()
            .take(k)
            .map(|(idx, score)| {
                let entry = &self.entries[idx];
                SearchResult {
                    chunk: entry.chunk.clone(),
                    score,
                    embedding: Some(crate::types::Embedding::new(entry.embedding.clone())),
                }
            })
            .collect();

        Ok(results)
    }

    fn delete(&mut self, id: &str) -> Result<(), VectorStoreError> {
        let before = self.entries.len();
        self.entries.retain(|e| e.chunk.id.to_string() != id);
        if self.entries.len() == before {
            return Err(VectorStoreError::NotFound(id.to_string()));
        }
        Ok(())
    }

    fn clear(&mut self) {
        self.entries.clear();
    }
}

impl Default for InMemoryVectorStore {
    fn default() -> Self {
        Self::new(384) // 默认维度（如 all-MiniLM-L6-v2）
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DocumentId, DocumentMeta, Embedding, TextChunk};

    fn make_chunk(text: &str, vec: Vec<f32>) -> DocumentChunk {
        DocumentChunk {
            chunk: TextChunk {
                id: DocumentId::new(),
                text: text.into(),
                meta: DocumentMeta::default(),
                start_offset: 0,
                end_offset: text.len(),
                document_id: DocumentId::new(),
                chunk_index: 0,
            },
            embedding: Some(Embedding::new(vec)),
        }
    }

    #[test]
    fn test_add_and_search() {
        let mut store = InMemoryVectorStore::new(4);

        store
            .add(make_chunk("rust", vec![1.0, 0.0, 0.0, 0.0]))
            .unwrap();
        store
            .add(make_chunk("python", vec![0.0, 1.0, 0.0, 0.0]))
            .unwrap();
        store
            .add(make_chunk("cooking", vec![0.0, 0.0, 1.0, 0.0]))
            .unwrap();

        assert_eq!(store.count(), 3);

        let results = store
            .search(&[1.0, 0.0, 0.0, 0.0], 2, DistanceMetric::Cosine)
            .unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].chunk.text, "rust");
        assert!((results[0].score - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_delete() {
        let mut store = InMemoryVectorStore::new(2);
        let chunk = make_chunk("test", vec![0.5, 0.5]);
        let id = chunk.chunk.id.to_string();
        store.add(chunk).unwrap();

        assert_eq!(store.count(), 1);
        store.delete(&id).unwrap();
        assert_eq!(store.count(), 0);
    }

    #[test]
    fn test_dimension_mismatch() {
        let mut store = InMemoryVectorStore::new(3);
        let result = store.add(make_chunk("bad", vec![0.1, 0.2]));
        assert!(result.is_err());
    }

    #[test]
    fn test_clear() {
        let mut store = InMemoryVectorStore::new(2);
        store.add(make_chunk("a", vec![1.0, 0.0])).unwrap();
        store.add(make_chunk("b", vec![0.0, 1.0])).unwrap();
        assert_eq!(store.count(), 2);
        store.clear();
        assert_eq!(store.count(), 0);
    }
}