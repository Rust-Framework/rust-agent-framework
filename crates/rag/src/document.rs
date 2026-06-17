use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::types::{Document, DocumentId, DocumentMeta, TextChunk};

// ─── 公共别名 ───────────────────────────────────────────

/// `crate::Result` 类型别名
pub type Result<T> = std::result::Result<T, DocumentError>;

/// 文档处理错误
#[derive(Debug, thiserror::Error)]
pub enum DocumentError {
    #[error("I/O 错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("解析错误: {0}")]
    Parse(String),

    #[error("不支持的格式: {0}")]
    UnsupportedFormat(String),
}

// ─── 文档加载器 trait ──────────────────────────────────

/// 文档加载器接口
#[async_trait]
pub trait DocumentLoader: Send + Sync {
    fn name(&self) -> &str;

    /// 从路径加载文档
    async fn load(&self, path: &str) -> Result<Document>;

    /// 从字符串内容加载文档
    async fn load_from_str(&self, content: &str, source: &str) -> Result<Document>;
}

// ─── 文本加载器 ─────────────────────────────────────────

/// 纯文本加载器
#[derive(Default)]
pub struct TextLoader;

#[async_trait]
impl DocumentLoader for TextLoader {
    fn name(&self) -> &str {
        "text-loader"
    }

    async fn load(&self, path: &str) -> Result<Document> {
        let content = tokio::fs::read_to_string(path).await?;
        Ok(Document::with_meta(
            content,
            DocumentMeta {
                source: Some(path.to_string()),
                doc_type: Some("text".into()),
                ..Default::default()
            },
        ))
    }

    async fn load_from_str(&self, content: &str, source: &str) -> Result<Document> {
        Ok(Document::with_meta(
            content.to_string(),
            DocumentMeta {
                source: Some(source.to_string()),
                doc_type: Some("text".into()),
                ..Default::default()
            },
        ))
    }
}

// ─── HTML 加载器 ────────────────────────────────────────

/// HTML 文档加载器 — 提取纯文本内容
#[derive(Default)]
pub struct HtmlLoader;

#[async_trait]
impl DocumentLoader for HtmlLoader {
    fn name(&self) -> &str {
        "html-loader"
    }

    async fn load(&self, path: &str) -> Result<Document> {
        let content = tokio::fs::read_to_string(path).await?;
        self.load_from_str(&content, path).await
    }

    async fn load_from_str(&self, content: &str, source: &str) -> Result<Document> {
        let text = extract_html_text(content)?;
        Ok(Document::with_meta(
            text,
            DocumentMeta {
                source: Some(source.to_string()),
                doc_type: Some("html".into()),
                ..Default::default()
            },
        ))
    }
}

/// 从 HTML 中提取纯文本（简单实现）
fn extract_html_text(html: &str) -> Result<String> {
    let document = scraper::Html::parse_document(html);

    // 收集 body 内的文本
    let body_selector = scraper::Selector::parse("body")
        .map_err(|e| DocumentError::Parse(e.to_string()))?;

    if let Some(body) = document.select(&body_selector).next() {
        let mut text = body.text().collect::<Vec<_>>().join(" ");
        // 压缩空白
        text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        Ok(text)
    } else {
        return Err(DocumentError::Parse(
            "HTML document has no <body> element".to_string(),
        ));
    }
}

// ─── 文档加载便捷函数 ──────────────────────────────────

/// 根据文件扩展名自动选择合适的加载器并加载文档
pub async fn load_document(path: &str) -> Result<Document> {
    let loader: Arc<dyn DocumentLoader> = if path.ends_with(".html") || path.ends_with(".htm") {
        Arc::new(HtmlLoader)
    } else {
        Arc::new(TextLoader)
    };
    loader.load(path).await
}

// ─── 分块策略 ──────────────────────────────────────────

/// 块重叠策略
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChunkOverlapStrategy {
    /// 固定重叠字符数
    Fixed(usize),
    /// 重叠比例（0.0 ~ 1.0）
    Ratio(f64),
}

impl Default for ChunkOverlapStrategy {
    fn default() -> Self {
        Self::Fixed(0)
    }
}

/// 分块策略
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChunkStrategy {
    /// 按固定字符数分块
    Fixed {
        chunk_size: usize,
        overlap: ChunkOverlapStrategy,
    },
    /// 递归字符分割（按段落 → 句子 → 词）
    Recursive {
        chunk_size: usize,
        chunk_overlap: usize,
    },
    /// 语义分块（按换行符分割）
    Semantic {
        max_chunk_size: usize,
    },
}

impl Default for ChunkStrategy {
    fn default() -> Self {
        Self::Recursive {
            chunk_size: 1000,
            chunk_overlap: 200,
        }
    }
}

/// 分块器 trait
pub trait Chunker: Send + Sync {
    fn name(&self) -> &str;

    /// 将文档分割为文本块
    fn chunk(&self, document: &Document) -> Vec<TextChunk>;

    /// 将文本直接分割为文本块
    fn chunk_text(&self, text: &str, source: Option<&str>) -> Vec<TextChunk>;
}

// ─── 递归字符分块器 ────────────────────────────────────

/// 递归字符分块器 — 按段落 → 句子层级递归分割
#[derive(Debug, Clone)]
pub struct RecursiveCharacterChunker {
    pub chunk_size: usize,
    pub chunk_overlap: usize,
    pub separators: Vec<String>,
}

impl Default for RecursiveCharacterChunker {
    fn default() -> Self {
        Self {
            chunk_size: 1000,
            chunk_overlap: 200,
            separators: vec![
                "\n\n".to_string(),
                "\n".to_string(),
                ".".to_string(),
                "!".to_string(),
                "?".to_string(),
                " ".to_string(),
                "".to_string(),
            ],
        }
    }
}

impl RecursiveCharacterChunker {
    pub fn new(chunk_size: usize, chunk_overlap: usize) -> Self {
        Self {
            chunk_size,
            chunk_overlap,
            ..Default::default()
        }
    }
}

impl Chunker for RecursiveCharacterChunker {
    fn name(&self) -> &str {
        "recursive-character-chunker"
    }

    fn chunk(&self, document: &Document) -> Vec<TextChunk> {
        self.chunk_text(&document.content, Some(&document.id.to_string()))
    }

    fn chunk_text(&self, text: &str, source: Option<&str>) -> Vec<TextChunk> {
        if text.is_empty() {
            return vec![];
        }

        let mut chunks = Vec::new();
        let mut start = 0usize;

        while start < text.len() {
            let end = if start + self.chunk_size >= text.len() {
                text.len()
            } else {
                // 尝试在分隔符处断开
                let end = start + self.chunk_size;
                self.find_split_point(text, end)
            };

            let chunk_text = &text[start..end];
            chunks.push(TextChunk {
                id: DocumentId::new(),
                text: chunk_text.to_string(),
                meta: DocumentMeta {
                    source: source.map(|s| s.to_string()),
                    ..Default::default()
                },
                start_offset: start,
                end_offset: end,
                document_id: DocumentId::new(),
                chunk_index: chunks.len(),
            });

            // 计算下一步的起始位置（带重叠）
            if end >= text.len() {
                break;
            }
            start = end.saturating_sub(self.chunk_overlap);
        }

        chunks
    }
}

impl RecursiveCharacterChunker {
    fn find_split_point(&self, text: &str, target: usize) -> usize {
        let mut best = target;

        for sep in &self.separators {
            if sep.is_empty() {
                break; // 最后一个分隔符是空字符串，表示字符级分割
            }
            if let Some(pos) = text[..target].rfind(sep) {
                let split_at = pos + sep.len();
                if split_at > target.saturating_sub(self.chunk_size / 4) {
                    best = split_at;
                    return best;
                }
            }
        }

        // 回退到目标位置
        best.min(text.len())
    }
}

// ─── 语义分块器 ────────────────────────────────────────

/// 语义分块器 — 按段落（双换行）分割，合并不超过 max_chunk_size
#[derive(Debug, Clone)]
pub struct SemanticChunker {
    pub max_chunk_size: usize,
}

impl Default for SemanticChunker {
    fn default() -> Self {
        Self { max_chunk_size: 2000 }
    }
}

impl SemanticChunker {
    pub fn new(max_chunk_size: usize) -> Self {
        Self { max_chunk_size }
    }
}

impl Chunker for SemanticChunker {
    fn name(&self) -> &str {
        "semantic-chunker"
    }

    fn chunk(&self, document: &Document) -> Vec<TextChunk> {
        self.chunk_text(&document.content, Some(&document.id.to_string()))
    }

    fn chunk_text(&self, text: &str, source: Option<&str>) -> Vec<TextChunk> {
        let paragraphs: Vec<&str> = text.split("\n\n").collect();
        let mut chunks = Vec::new();
        let mut current = String::new();
        let mut current_start = 0usize;

        for para in &paragraphs {
            if para.trim().is_empty() {
                continue;
            }

            if current.len() + para.len() + 2 > self.max_chunk_size && !current.is_empty() {
                let chunk_text = std::mem::take(&mut current);
                let chunk_end = current_start + chunk_text.len();
                chunks.push(TextChunk {
                    id: DocumentId::new(),
                    text: chunk_text,
                    meta: DocumentMeta {
                        source: source.map(|s| s.to_string()),
                        ..Default::default()
                    },
                    start_offset: current_start,
                    end_offset: chunk_end,
                    document_id: DocumentId::new(),
                    chunk_index: chunks.len(),
                });
                current_start = chunk_end;
            }

            if current.is_empty() {
                current_start = text[current_start..].find(para)
                    .map(|pos| current_start + pos)
                    .unwrap_or(0);
            }

            if !current.is_empty() {
                current.push_str("\n\n");
            }
            current.push_str(para);
        }

        // last chunk
        if !current.is_empty() {
            let chunk_end = current_start + current.len();
            chunks.push(TextChunk {
                id: DocumentId::new(),
                text: current,
                meta: DocumentMeta {
                    source: source.map(|s| s.to_string()),
                    ..Default::default()
                },
                start_offset: current_start,
                end_offset: chunk_end,
                document_id: DocumentId::new(),
                chunk_index: chunks.len(),
            });
        }

        chunks
    }
}

// ─── Chunker 枚举封装 ─────────────────────────────────

impl Chunker for ChunkStrategy {
    fn name(&self) -> &str {
        match self {
            Self::Fixed { .. } => "fixed-chunker",
            Self::Recursive { .. } => "recursive-chunker",
            Self::Semantic { .. } => "semantic-chunker",
        }
    }

    fn chunk(&self, document: &Document) -> Vec<TextChunk> {
        match self {
            Self::Fixed {
                chunk_size,
                overlap,
            } => {
                let overlap_chars = match overlap {
                    ChunkOverlapStrategy::Fixed(n) => *n,
                    ChunkOverlapStrategy::Ratio(r) => (*r * *chunk_size as f64) as usize,
                };
                let chunker = RecursiveCharacterChunker::new(*chunk_size, overlap_chars);
                chunker.chunk(document)
            }
            Self::Recursive {
                chunk_size,
                chunk_overlap,
            } => {
                let chunker =
                    RecursiveCharacterChunker::new(*chunk_size, *chunk_overlap);
                chunker.chunk(document)
            }
            Self::Semantic { max_chunk_size } => {
                let chunker = SemanticChunker::new(*max_chunk_size);
                chunker.chunk(document)
            }
        }
    }

    fn chunk_text(&self, text: &str, source: Option<&str>) -> Vec<TextChunk> {
        match self {
            Self::Fixed {
                chunk_size,
                overlap,
            } => {
                let overlap_chars = match overlap {
                    ChunkOverlapStrategy::Fixed(n) => *n,
                    ChunkOverlapStrategy::Ratio(r) => (*r * *chunk_size as f64) as usize,
                };
                let chunker = RecursiveCharacterChunker::new(*chunk_size, overlap_chars);
                chunker.chunk_text(text, source)
            }
            Self::Recursive {
                chunk_size,
                chunk_overlap,
            } => {
                let chunker =
                    RecursiveCharacterChunker::new(*chunk_size, *chunk_overlap);
                chunker.chunk_text(text, source)
            }
            Self::Semantic { max_chunk_size } => {
                let chunker = SemanticChunker::new(*max_chunk_size);
                chunker.chunk_text(text, source)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_text_loader() {
        let loader = TextLoader;
        let doc = loader.load_from_str("Hello, world!", "test").await.unwrap();
        assert_eq!(doc.content, "Hello, world!");
        assert_eq!(doc.meta.source, Some("test".into()));
    }

    #[test]
    fn test_recursive_chunker_fixed_size() {
        let chunker = RecursiveCharacterChunker::new(10, 2);
        let doc = Document::new("Hello world this is a test document for chunking");
        let chunks = chunker.chunk(&doc);
        assert!(!chunks.is_empty());
    }

    #[test]
    fn test_semantic_chunker() {
        let chunker = SemanticChunker::new(100);
        let text = "Paragraph one.\n\nParagraph two.\n\nParagraph three is longer.";
        let doc = Document::new(text);
        let chunks = chunker.chunk(&doc);
        assert!(!chunks.is_empty());
    }

    #[test]
    fn test_chunk_strategy_enum() {
        let strategy = ChunkStrategy::Recursive {
            chunk_size: 500,
            chunk_overlap: 50,
        };
        let doc = Document::new("A".repeat(1000));
        let chunks = strategy.chunk(&doc);
        assert!(chunks.len() >= 2);
    }

    #[tokio::test]
    async fn test_html_loader_extract_text() {
        let loader = HtmlLoader;
        let html = "<html><body><p>Hello <b>world</b></p></body></html>";
        let doc = loader.load_from_str(html, "test.html").await.unwrap();
        assert!(doc.content.contains("Hello world") || doc.content.contains("Hello  world"));
    }

    #[tokio::test]
    async fn test_load_document_auto_select() {
        // text file
        let result = load_document("Cargo.toml").await;
        assert!(result.is_ok());
    }
}