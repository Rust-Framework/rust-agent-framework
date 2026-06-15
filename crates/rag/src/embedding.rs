use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::types::{DocumentChunk, Embedding, TextChunk};

// ─── 嵌入模型 trait ─────────────────────────────────────

/// 嵌入模型接口 — 将文本转换为向量
#[async_trait]
pub trait IEmbeddingModel: Send + Sync {
    /// 模型名称
    fn name(&self) -> &str;

    /// 嵌入维度
    fn dimensions(&self) -> usize;

    /// 对单段文本进行嵌入
    async fn embed(&self, text: &str) -> crate::Result<Embedding>;

    /// 对多段文本进行批量嵌入
    async fn embed_batch(&self, texts: &[&str]) -> crate::Result<Vec<Embedding>>;
}

// ─── EmbeddingModel 枚举（内置实现 + 扩展点）────────────

/// 嵌入模型枚举 — 提供开箱即用的内置实现
///
/// 可以通过 `Arc<dyn IEmbeddingModel>` 传入自定义模型来扩展。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EmbeddingModel {
    /// 简单的词袋/TF-IDF 类型嵌入（仅用于测试/演示）
    ///
    /// 将文本按 unicode 词分割并为每个唯一词分配一个伪随机维度索引，
    /// 生成稀疏向量。这不是真正的语义嵌入，仅用于功能验证。
    Simple { dimensions: usize },
}

#[async_trait]
impl IEmbeddingModel for EmbeddingModel {
    fn name(&self) -> &str {
        match self {
            Self::Simple { .. } => "simple-embedding",
        }
    }

    fn dimensions(&self) -> usize {
        match self {
            Self::Simple { dimensions } => *dimensions,
        }
    }

    async fn embed(&self, text: &str) -> crate::Result<Embedding> {
        match self {
            Self::Simple { dimensions } => Ok(simple_embed(text, *dimensions)),
        }
    }

    async fn embed_batch(&self, texts: &[&str]) -> crate::Result<Vec<Embedding>> {
        match self {
            Self::Simple { dimensions } => {
                Ok(texts.iter().map(|t| simple_embed(t, *dimensions)).collect())
            }
        }
    }
}

/// 简单哈希嵌入 — 用词频构造稀疏向量
fn simple_embed(text: &str, dimensions: usize) -> Embedding {
    use std::collections::HashMap;
    use unicode_segmentation::UnicodeSegmentation;

    let mut freq: HashMap<u64, f32> = HashMap::new();
    let words: Vec<&str> = text.graphemes(true).collect();
    let word_count = words.len() as f32;

    for word in &words {
        let hash = word.chars().fold(0u64, |mut h, c| {
            h = h.wrapping_mul(31).wrapping_add(c as u64);
            h
        });
        let idx = (hash as usize) % dimensions;
        *freq.entry(idx as u64).or_insert(0.0) += 1.0 / word_count;
    }

    let mut vector = vec![0.0f32; dimensions];
    for (idx, val) in &freq {
        vector[*idx as usize] = *val;
    }

    Embedding::new(vector)
}

// ─── 便捷函数 ───────────────────────────────────────────

/// 创建简单的测试用嵌入模型
pub fn simple_embedding_model(dimensions: usize) -> EmbeddingModel {
    EmbeddingModel::Simple { dimensions }
}

/// 批量给文档块生成嵌入
pub async fn embed_chunks(
    model: &dyn IEmbeddingModel,
    chunks: Vec<TextChunk>,
) -> crate::Result<Vec<DocumentChunk>> {
    let texts: Vec<&str> = chunks.iter().map(|c| c.text.as_str()).collect();
    let embeddings = model.embed_batch(&texts).await?;

    Ok(chunks
        .into_iter()
        .zip(embeddings)
        .map(|(chunk, embedding)| DocumentChunk {
            chunk,
            embedding: Some(embedding),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DocumentId, DocumentMeta};

    #[tokio::test]
    async fn test_simple_embedding() {
        let model = simple_embedding_model(64);
        let emb = model.embed("hello world").await.unwrap();
        assert_eq!(emb.dimensions, 64);
        assert!(emb.vector.iter().any(|&x| x > 0.0));
    }

    #[tokio::test]
    async fn test_cosine_similarity() {
        let model = simple_embedding_model(64);
        let a = model.embed("rust programming language").await.unwrap();
        let b = model.embed("rust programming language").await.unwrap();
        let c = model.embed("cooking recipes").await.unwrap();

        let sim_ab = a.cosine_similarity(&b);
        let sim_ac = a.cosine_similarity(&c);

        assert!(sim_ab > 0.99, "identical texts should have ~1.0 similarity");
        assert!(
            sim_ac < sim_ab,
            "different texts should have lower similarity"
        );
    }

    #[tokio::test]
    async fn test_embed_chunks() {
        let model = simple_embedding_model(32);
        let chunks = vec![
            TextChunk {
                id: DocumentId::new(),
                text: "chunk one".into(),
                meta: DocumentMeta::default(),
                start_offset: 0,
                end_offset: 9,
                document_id: DocumentId::new(),
                chunk_index: 0,
            },
            TextChunk {
                id: DocumentId::new(),
                text: "chunk two".into(),
                meta: DocumentMeta::default(),
                start_offset: 10,
                end_offset: 18,
                document_id: DocumentId::new(),
                chunk_index: 1,
            },
        ];

        let doc_chunks = embed_chunks(&model, chunks).await.unwrap();
        assert_eq!(doc_chunks.len(), 2);
        assert!(doc_chunks[0].embedding.is_some());
        assert!(doc_chunks[1].embedding.is_some());
    }
}