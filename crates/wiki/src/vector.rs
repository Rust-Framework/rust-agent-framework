//! 向量索引 —— v2 混合检索的语义召回层。
//!
//! 基于 `rust-agent-rag` 的 `IVectorStore` + `IEmbeddingModel`，将 wiki 页面分块、
//! 嵌入并索引，支持按查询向量检索语义相似的页面块。
//!
//! 设计要点：
//! - 每个 wiki 空间持有一个 `VectorIndex`，按 slug 维护"页面 → 块 ID"映射。
//! - 写入（`index_page` / `remove_page`）通过 `Mutex` 串行化，读取（`search`）并发安全。
//! - 嵌入模型通过 trait 注入，生产环境可替换为真实语义模型。

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use rust_agent_rag::vector_store::{DistanceMetric, InMemoryVectorStore, IVectorStore};
use rust_agent_rag::embedding::IEmbeddingModel;
use rust_agent_rag::types::{DocumentChunk, DocumentId, DocumentMeta, SearchResult, TextChunk};

use crate::frontmatter;
use crate::markdown;

/// 默认嵌入维度（与 `InMemoryVectorStore::default` 对齐）。
const DEFAULT_DIMENSIONS: usize = 384;

/// 单页分块的目标字符数。
const CHUNK_SIZE: usize = 512;

/// 单页分块的字符重叠量。
const CHUNK_OVERLAP: usize = 64;

/// 一个向量召回结果，映射回 wiki 页面。
#[derive(Debug, Clone)]
pub struct VectorHit {
    /// 命中块所属页面的 slug。
    pub slug: String,
    /// 页面标题。
    pub title: String,
    /// 相似度分数（余弦相似度，[-1, 1] 归一化到 [0, 1]）。
    pub score: f32,
    /// 命中的文本片段。
    pub snippet: String,
}

/// 向量索引 —— 包装 `InMemoryVectorStore` + 嵌入模型，按 slug 组织页面块。
pub struct VectorIndex {
    store: Mutex<InMemoryVectorStore>,
    embed_model: Arc<dyn IEmbeddingModel>,
    /// slug → 该页面的所有块 ID（用于按页删除）。
    slug_chunks: Mutex<HashMap<String, Vec<DocumentId>>>,
    /// slug → 页面标题（用于召回结果回填）。
    slug_titles: Mutex<HashMap<String, String>>,
    dimensions: usize,
}

impl VectorIndex {
    /// 用指定的嵌入模型和维度创建一个空索引。
    pub fn new(embed_model: Arc<dyn IEmbeddingModel>, dimensions: usize) -> Self {
        Self {
            store: Mutex::new(InMemoryVectorStore::new(dimensions)),
            embed_model,
            slug_chunks: Mutex::new(HashMap::new()),
            slug_titles: Mutex::new(HashMap::new()),
            dimensions,
        }
    }

    /// 用默认维度（384）创建。
    pub fn with_default_dimensions(embed_model: Arc<dyn IEmbeddingModel>) -> Self {
        Self::new(embed_model, DEFAULT_DIMENSIONS)
    }

    /// 返回嵌入维度。
    pub fn dimensions(&self) -> usize {
        self.dimensions
    }

    /// 返回当前索引的页面数（按 slug 计）。
    pub fn page_count(&self) -> usize {
        self.slug_chunks.lock().len()
    }

    /// 返回当前索引的块数。
    pub fn chunk_count(&self) -> usize {
        self.store.lock().count()
    }

    /// 索引一个页面：解析 frontmatter 取标题，分块 body，嵌入后写入向量存储。
    ///
    /// 若该 slug 已存在，先移除旧块再写入新块（upsert 语义）。
    pub async fn index_page(&self, slug: &str, content: &str) -> anyhow::Result<()> {
        let parsed = frontmatter::parse(content);
        let title = parsed
            .title()
            .map(|s| s.to_string())
            .unwrap_or_else(|| slug.to_string());
        let body = &parsed.body;

        // upsert：先移除旧块
        self.remove_page(slug);

        let chunks = chunk_body(body, slug);
        if chunks.is_empty() {
            // 仍记录标题，便于空页面被 slug 检索
            self.slug_titles.lock().insert(slug.to_string(), title);
            self.slug_chunks.lock().insert(slug.to_string(), Vec::new());
            return Ok(());
        }

        let texts: Vec<&str> = chunks.iter().map(|c| c.text.as_str()).collect();
        let embeddings = self
            .embed_model
            .embed_batch(&texts)
            .await
            .map_err(|e| anyhow::anyhow!("embedding failed: {e}"))?;

        let mut chunk_ids = Vec::with_capacity(chunks.len());
        {
            let mut store = self.store.lock();
            for (chunk, embedding) in chunks.into_iter().zip(embeddings) {
                let id_str = chunk.id.to_string();
                let doc_chunk = DocumentChunk {
                    chunk,
                    embedding: Some(embedding),
                };
                store.add(doc_chunk)?;
                chunk_ids.push(rust_agent_rag::types::DocumentId::from(id_str));
            }
        }

        self.slug_titles.lock().insert(slug.to_string(), title);
        self.slug_chunks.lock().insert(slug.to_string(), chunk_ids);
        Ok(())
    }

    /// 从磁盘读取并索引一个页面文件。
    pub async fn index_page_file(&self, slug: &str, wiki_root: &std::path::Path) -> anyhow::Result<()> {
        let slug_obj = crate::slug::Slug::try_from(slug)?;
        let content = markdown::read_page(&slug_obj, wiki_root, false)?;
        self.index_page(slug, &content).await
    }

    /// 移除一个页面的所有块。
    pub fn remove_page(&self, slug: &str) {
        let chunk_ids = self.slug_chunks.lock().remove(slug);
        if let Some(ids) = chunk_ids {
            let mut store = self.store.lock();
            for id in ids {
                let _ = store.delete(&id.to_string());
            }
        }
        self.slug_titles.lock().remove(slug);
    }

    /// 清空整个索引。
    pub fn clear(&self) {
        self.store.lock().clear();
        self.slug_chunks.lock().clear();
        self.slug_titles.lock().clear();
    }

    /// 语义检索：嵌入查询，在向量存储中搜索 top-k 块，按 slug 聚合。
    ///
    /// 同一 slug 的多个块取最高分作为该页的代表分数。
    pub async fn search(&self, query: &str, k: usize) -> anyhow::Result<Vec<VectorHit>> {
        if self.chunk_count() == 0 {
            return Ok(Vec::new());
        }
        let query_emb = self
            .embed_model
            .embed(query)
            .await
            .map_err(|e| anyhow::anyhow!("query embedding failed: {e}"))?;
        let raw: Vec<SearchResult> = {
            let store = self.store.lock();
            store.search(&query_emb.vector, k * 2, DistanceMetric::Cosine)?
        };

        let titles = self.slug_titles.lock();
        let mut by_slug: HashMap<String, VectorHit> = HashMap::new();
        for r in raw {
            let slug = r.chunk.meta.custom.get("slug").cloned().unwrap_or_default();
            if slug.is_empty() {
                continue;
            }
            let title = titles.get(&slug).cloned().unwrap_or_default();
            let score = (r.score as f32).clamp(0.0, 1.0);
            let entry = by_slug.entry(slug.clone()).or_insert_with(|| VectorHit {
                slug: slug.clone(),
                title: title.clone(),
                score,
                snippet: r.chunk.text.clone(),
            });
            // 取最高分
            if r.score as f32 > entry.score {
                entry.score = score;
                entry.snippet = r.chunk.text.clone();
            }
            if by_slug.len() >= k {
                break;
            }
        }

        let mut hits: Vec<VectorHit> = by_slug.into_values().collect();
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        hits.truncate(k);
        Ok(hits)
    }
}

impl std::fmt::Debug for VectorIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VectorIndex")
            .field("dimensions", &self.dimensions)
            .field("pages", &self.page_count())
            .field("chunks", &self.chunk_count())
            .finish()
    }
}

// ── 分块 ──────────────────────────────────────────────────────────────────────

/// 将 body 文本按字符数分块（带重叠），每块标注 slug 元数据。
fn chunk_body(body: &str, slug: &str) -> Vec<TextChunk> {
    let body = body.trim();
    if body.is_empty() {
        return Vec::new();
    }
    let chars: Vec<char> = body.chars().collect();
    let mut chunks = Vec::new();
    let mut start = 0;
    let mut idx = 0;
    while start < chars.len() {
        let end = (start + CHUNK_SIZE).min(chars.len());
        let text: String = chars[start..end].iter().collect();
        let mut meta = DocumentMeta::default();
        meta.custom.insert("slug".to_string(), slug.to_string());
        chunks.push(TextChunk {
            id: DocumentId::new(),
            text,
            meta,
            start_offset: start,
            end_offset: end,
            document_id: DocumentId::new(),
            chunk_index: idx,
        });
        idx += 1;
        if end == chars.len() {
            break;
        }
        start = end.saturating_sub(CHUNK_OVERLAP);
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_agent_rag::embedding::simple_embedding_model;

    #[test]
    fn test_chunk_body_basic() {
        let body = "a".repeat(1200);
        let chunks = chunk_body(&body, "test/slug");
        assert!(chunks.len() > 1);
        assert_eq!(chunks[0].meta.custom.get("slug"), Some(&"test/slug".to_string()));
        assert_eq!(chunks[0].chunk_index, 0);
    }

    #[test]
    fn test_chunk_body_empty() {
        assert!(chunk_body("", "x").is_empty());
        assert!(chunk_body("   \n\n  ", "x").is_empty());
    }

    #[tokio::test]
    async fn test_index_and_search() {
        let model = Arc::new(simple_embedding_model(32));
        // 用 32 维重建
        let idx = VectorIndex::new(Arc::clone(&model) as Arc<dyn IEmbeddingModel>, 32);

        let page_a = "---\ntitle: Rust Ownership\ntype: concept\n---\n# Ownership\nRust ownership model ensures memory safety without GC.";
        let page_b = "---\ntitle: Cooking Pasta\ntype: doc\n---\n# Pasta\nBoil water and cook pasta for 10 minutes.";

        idx.index_page("concepts/rust-ownership", page_a).await.unwrap();
        idx.index_page("docs/cooking-pasta", page_b).await.unwrap();

        assert_eq!(idx.page_count(), 2);

        let hits = idx.search("rust memory safety", 5).await.unwrap();
        assert!(!hits.is_empty());
        // 最相关的应该是 rust ownership
        assert_eq!(hits[0].slug, "concepts/rust-ownership");
    }

    #[tokio::test]
    async fn test_remove_page() {
        let model = Arc::new(simple_embedding_model(16));
        let idx = VectorIndex::new(Arc::clone(&model) as Arc<dyn IEmbeddingModel>, 16);
        idx.index_page("a", "---\ntitle: A\n---\nhello world").await.unwrap();
        assert_eq!(idx.page_count(), 1);
        idx.remove_page("a");
        assert_eq!(idx.page_count(), 0);
        assert_eq!(idx.chunk_count(), 0);
    }

    #[tokio::test]
    async fn test_upsert() {
        let model = Arc::new(simple_embedding_model(16));
        let idx = VectorIndex::new(Arc::clone(&model) as Arc<dyn IEmbeddingModel>, 16);
        idx.index_page("a", "---\ntitle: A\n---\nfirst version").await.unwrap();
        let n1 = idx.chunk_count();
        idx.index_page("a", "---\ntitle: A\n---\nsecond version completely different").await.unwrap();
        let n2 = idx.chunk_count();
        // upsert 不应导致块数翻倍
        assert!(n2 <= n1 + 1);
        assert_eq!(idx.page_count(), 1);
    }
}
