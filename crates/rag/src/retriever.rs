use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::types::SearchResult;
use crate::vector_store::{DistanceMetric, IVectorStore, InMemoryVectorStore};

/// 检索选项
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrieverOptions {
    /// 返回 Top-K 结果
    pub k: usize,
    /// 距离度量
    pub metric: DistanceMetric,
    /// 最低分数阈值（低于此值的将被过滤）
    pub score_threshold: Option<f64>,
}

impl Default for RetrieverOptions {
    fn default() -> Self {
        Self {
            k: 4,
            metric: DistanceMetric::Cosine,
            score_threshold: None,
        }
    }
}

/// 检索器返回结果
#[derive(Debug, Clone)]
pub struct RetrieverResult {
    pub results: Vec<SearchResult>,
    pub total_time_ms: u64,
}

/// 检索器接口
#[async_trait]
pub trait IRetriever: Send + Sync {
    fn name(&self) -> &str;

    /// 执行检索
    async fn retrieve(
        &self,
        query: &str,
        options: &RetrieverOptions,
    ) -> crate::Result<RetrieverResult>;
}

// ─── 相似度检索器 ──────────────────────────────────────

/// 相似度检索器 — 使用向量相似度搜索
pub struct SimilarityRetriever {
    vector_store: Arc<dyn IVectorStore>,
    embed_model: Arc<dyn crate::embedding::IEmbeddingModel>,
}

impl SimilarityRetriever {
    pub fn new(
        vector_store: Arc<dyn IVectorStore>,
        embed_model: Arc<dyn crate::embedding::IEmbeddingModel>,
    ) -> Self {
        Self {
            vector_store,
            embed_model,
        }
    }

    /// 从 InMemoryVectorStore 构建
    pub fn from_inmemory(
        store: InMemoryVectorStore,
        embed_model: impl crate::embedding::IEmbeddingModel + 'static,
    ) -> Self {
        Self {
            vector_store: Arc::new(store),
            embed_model: Arc::new(embed_model),
        }
    }
}

#[async_trait]
impl IRetriever for SimilarityRetriever {
    fn name(&self) -> &str {
        "similarity-retriever"
    }

    async fn retrieve(
        &self,
        query: &str,
        options: &RetrieverOptions,
    ) -> crate::Result<RetrieverResult> {
        let start = std::time::Instant::now();

        // 1. 对查询进行嵌入
        let query_embedding = self.embed_model.embed(query).await?;

        // 2. 在向量存储中搜索
        let results = self.vector_store.search(
            &query_embedding.vector,
            options.k,
            options.metric,
        )?;

        // 3. 应用分数阈值过滤
        let results = if let Some(threshold) = options.score_threshold {
            results
                .into_iter()
                .filter(|r| r.score >= threshold)
                .collect()
        } else {
            results
        };

        let elapsed = start.elapsed().as_millis() as u64;

        Ok(RetrieverResult {
            results,
            total_time_ms: elapsed,
        })
    }
}

// ─── Retriever 枚举 ────────────────────────────────────

/// 检索器枚举
pub enum Retriever {
    /// 相似度检索
    Similarity(SimilarityRetriever),
}

#[async_trait]
impl IRetriever for Retriever {
    fn name(&self) -> &str {
        match self {
            Self::Similarity(r) => r.name(),
        }
    }

    async fn retrieve(
        &self,
        query: &str,
        options: &RetrieverOptions,
    ) -> crate::Result<RetrieverResult> {
        match self {
            Self::Similarity(r) => r.retrieve(query, options).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::simple_embedding_model;
    use crate::types::{DocumentChunk, DocumentId, DocumentMeta, Embedding, TextChunk};
    use crate::vector_store::InMemoryVectorStore;

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

    #[tokio::test]
    async fn test_similarity_retriever() {
        let mut store = InMemoryVectorStore::new(4);
        store
            .add(make_chunk("rust programming", vec![1.0, 0.0, 0.0, 0.0]))
            .unwrap();
        store
            .add(make_chunk("python programming", vec![0.0, 1.0, 0.0, 0.0]))
            .unwrap();
        store
            .add(make_chunk("cooking recipes", vec![0.0, 0.0, 1.0, 0.0]))
            .unwrap();

        // 使用真实嵌入模型，但为简单测试直接使用 store
        let model = simple_embedding_model(4);
        let retriever = SimilarityRetriever::from_inmemory(store, model);

        // 测试时使用 store 的 search 方法直接验证
        let _options = RetrieverOptions::default();

        // 基本功能验证：通过 store 直接搜索
        let results = retriever
            .vector_store
            .search(&[1.0, 0.0, 0.0, 0.0], 2, DistanceMetric::Cosine)
            .unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_score_threshold() {
        let mut store = InMemoryVectorStore::new(2);
        store
            .add(make_chunk("a", vec![1.0, 0.0]))
            .unwrap();
        store
            .add(make_chunk("b", vec![0.0, 1.0]))
            .unwrap();

        let model = simple_embedding_model(2);
        let retriever = SimilarityRetriever {
            vector_store: Arc::new(store),
            embed_model: Arc::new(model),
        };

        let options = RetrieverOptions {
            k: 4,
            score_threshold: Some(0.5),
            ..Default::default()
        };

        // 用更接近 rust 的 query
        let result = retriever
            .retrieve("a", &options)
            .await
            .unwrap();
        // 至少应该返回一条结果
        assert!(!result.results.is_empty());
    }
}