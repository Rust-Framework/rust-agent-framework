use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// 文档唯一标识符
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DocumentId(pub String);

impl DocumentId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

impl From<String> for DocumentId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for DocumentId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl std::fmt::Display for DocumentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 文档元数据
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DocumentMeta {
    /// 标题
    pub title: Option<String>,
    /// 来源 URL 或文件路径
    pub source: Option<String>,
    /// 文档类型
    pub doc_type: Option<String>,
    /// 自定义元数据
    pub custom: HashMap<String, String>,
}

/// 原始文档
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: DocumentId,
    pub content: String,
    pub meta: DocumentMeta,
}

impl Document {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            id: DocumentId::new(),
            content: content.into(),
            meta: DocumentMeta::default(),
        }
    }

    pub fn with_meta(content: impl Into<String>, meta: DocumentMeta) -> Self {
        Self {
            id: DocumentId::new(),
            content: content.into(),
            meta,
        }
    }
}

/// 文本块 — 文档分块后的基本单元
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextChunk {
    pub id: DocumentId,
    pub text: String,
    pub meta: DocumentMeta,
    /// 在原始文档中的起始偏移（字符数）
    pub start_offset: usize,
    /// 在原始文档中的结束偏移（字符数）
    pub end_offset: usize,
    /// 所属文档 ID
    pub document_id: DocumentId,
    /// 块序号
    pub chunk_index: usize,
}

/// 文档块（包含向量嵌入）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentChunk {
    pub chunk: TextChunk,
    pub embedding: Option<Embedding>,
}

/// 嵌入向量
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Embedding {
    pub vector: Vec<f32>,
    pub dimensions: usize,
}

impl Embedding {
    pub fn new(vector: Vec<f32>) -> Self {
        let dimensions = vector.len();
        Self { vector, dimensions }
    }

    /// 计算余弦相似度
    pub fn cosine_similarity(&self, other: &Embedding) -> f64 {
        if self.dimensions != other.dimensions {
            return 0.0;
        }

        let dot: f32 = self.vector.iter().zip(&other.vector).map(|(a, b)| a * b).sum();
        let norm_a: f32 = self.vector.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = other.vector.iter().map(|x| x * x).sum::<f32>().sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            return 0.0;
        }

        (dot / (norm_a * norm_b)) as f64
    }
}

/// 检索结果
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub chunk: TextChunk,
    pub score: f64,
    pub embedding: Option<Embedding>,
}