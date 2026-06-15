//! # rust-agent-rag
//!
//! RAG (Retrieval-Augmented Generation) 支持库，为 RAF 框架提供文档加载、
//! 文本分块、向量嵌入、向量存储和检索能力。
//!
//! ## 核心组件
//!
//! - **`document`** — 文档加载、文本提取、分块策略
//! - **`embedding`** — 嵌入模型接口与向量化
//! - **`vector_store`** — 向量存储抽象与内存实现
//! - **`retriever`** — 检索策略（相似度搜索、MMR 等）
//! - **`types`** — 共享类型定义

pub mod document;
pub mod embedding;
pub mod retriever;
pub mod types;
pub mod vector_store;

// ── 重新导出核心公共 API ──

pub use document::{
    load_document, ChunkOverlapStrategy, ChunkStrategy, Chunker, DocumentLoader, DocumentError,
    HtmlLoader, RecursiveCharacterChunker, SemanticChunker, TextLoader,
};
pub use embedding::{simple_embedding_model, embed_chunks, EmbeddingModel, IEmbeddingModel};
pub use retriever::{
    IRetriever, Retriever, RetrieverOptions, RetrieverResult, SimilarityRetriever,
};
pub use types::{
    Document, DocumentChunk, DocumentId, DocumentMeta, Embedding, SearchResult, TextChunk,
};
pub use vector_store::{
    DistanceMetric, IVectorStore, InMemoryVectorStore, IndexEntry, VectorStore, VectorStoreError,
};

/// 便捷 Result 别名
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;