//! RAG 知识库上下文提供器 — 将检索结果注入 Agent 对话。

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::{
    ChatMessage, IContextProvider, MessageInjection, MessageRole, ProviderContext, Result,
};
use tokio::sync::OnceCell;
use walkdir::WalkDir;

use crate::{
    embed_chunks, simple_embedding_model, Chunker, DocumentLoader, IRetriever,
    RecursiveCharacterChunker, RetrieverOptions, SimilarityRetriever, TextLoader,
};
use crate::{InMemoryVectorStore, IVectorStore};

const DEFAULT_DIMENSIONS: usize = 128;
const DEFAULT_TOP_K: usize = 4;

/// 基于 RAG 检索的知识库上下文提供器。
///
/// 首次调用 `enrich_messages` 时异步加载/索引 `source`，随后按用户最新消息检索相关片段。
pub struct RagContextProvider {
    name: String,
    source: String,
    top_k: usize,
    retriever: OnceCell<Arc<dyn IRetriever>>,
}

impl RagContextProvider {
    pub fn new(name: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            source: source.into(),
            top_k: DEFAULT_TOP_K,
            retriever: OnceCell::new(),
        }
    }

    pub fn with_top_k(mut self, k: usize) -> Self {
        self.top_k = k;
        self
    }

    async fn retriever(&self) -> Result<Arc<dyn IRetriever>> {
        self.retriever
            .get_or_try_init(|| async {
                build_retriever(&self.source)
                    .await
                    .map(|r| Arc::new(r) as Arc<dyn IRetriever>)
                    .map_err(|e| rust_agent_core::AgentError::ConfigError(format!("RAG index: {e}")))
            })
            .await
            .map(Arc::clone)
    }

    fn last_user_query(messages: &[ChatMessage]) -> Option<String> {
        messages
            .iter()
            .rev()
            .find(|m| m.role == MessageRole::User)
            .map(|m| m.content.clone())
            .filter(|c| !c.trim().is_empty())
    }
}

#[async_trait]
impl IContextProvider for RagContextProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> &str {
        "knowledge"
    }

    async fn enrich_instructions(&self, _ctx: &ProviderContext<'_>) -> Result<Option<String>> {
        Ok(Some(format!(
            "## 知识库: {}\n\
             来源: {}\n\
             相关文档片段会在检索后自动注入到对话上下文。",
            self.name, self.source
        )))
    }

    async fn enrich_messages(&self, ctx: &ProviderContext<'_>) -> Result<MessageInjection> {
        let query = match Self::last_user_query(ctx.messages) {
            Some(q) => q,
            None => return Ok(MessageInjection::default()),
        };

        let retriever = self.retriever().await?;
        let options = RetrieverOptions {
            k: self.top_k,
            ..Default::default()
        };
        let result = retriever
            .retrieve(&query, &options)
            .await
            .map_err(|e| rust_agent_core::AgentError::ConfigError(format!("RAG retrieve: {e}")))?;

        if result.results.is_empty() {
            return Ok(MessageInjection::default());
        }

        let mut body = String::from("## 检索到的相关知识\n\n");
        for (i, hit) in result.results.iter().enumerate() {
            body.push_str(&format!(
                "### 片段 {}\n{}\n\n",
                i + 1,
                hit.chunk.text.trim()
            ));
        }

        Ok(MessageInjection {
            messages: vec![ChatMessage::system(body)],
            replace: false,
        })
    }
}

async fn build_retriever(source: &str) -> crate::Result<SimilarityRetriever> {
    let loader = TextLoader;
    let chunker = RecursiveCharacterChunker::default();
    let model = simple_embedding_model(DEFAULT_DIMENSIONS);
    let mut store = InMemoryVectorStore::new(DEFAULT_DIMENSIONS);

    let path = Path::new(source);
    if path.is_dir() {
        for entry in WalkDir::new(path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let p = entry.path().to_string_lossy();
            if let Ok(doc) = loader.load(&p).await {
                index_document(&chunker, &model, &mut store, &doc.content).await?;
            }
        }
    } else if !source.is_empty() {
        let doc = loader.load(source).await?;
        index_document(&chunker, &model, &mut store, &doc.content).await?;
    }

    Ok(SimilarityRetriever::new(
        Arc::new(store) as Arc<dyn IVectorStore>,
        Arc::new(model),
    ))
}

async fn index_document(
    chunker: &RecursiveCharacterChunker,
    model: &crate::EmbeddingModel,
    store: &mut InMemoryVectorStore,
    content: &str,
) -> crate::Result<()> {
    let text_chunks = chunker.chunk_text(content, None);
    let doc_chunks = embed_chunks(model, text_chunks).await?;
    store
        .add_batch(doc_chunks)
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::build_retriever;
    use crate::IRetriever;

    #[tokio::test]
    async fn indexes_directory_source() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("kb.txt");
        std::fs::write(&file, "Rust agent framework supports RAG retrieval.").unwrap();

        let retriever = build_retriever(&dir.path().to_string_lossy())
            .await
            .expect("build retriever");
        let result = retriever
            .retrieve("RAG retrieval", &Default::default())
            .await
            .expect("retrieve");
        assert!(!result.results.is_empty());
    }
}
