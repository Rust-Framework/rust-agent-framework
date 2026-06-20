//! Wiki 知识库上下文提供器 — BM25 全文检索注入 Agent 对话。
//!
//! v2: 可选启用混合检索（BM25 + 向量 + 图遍历，RRF 融合）。

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::{
    ChatMessage, IContextProvider, MessageInjection, MessageRole, ProviderContext, Result,
};
use rust_agent_wiki::ops::{search as wiki_search, SearchParams};
use rust_agent_wiki::WikiEngine;
use tokio::sync::OnceCell;

const DEFAULT_TOP_K: usize = 5;

/// 基于 `rust-agent-wiki` 的上下文提供器。
pub struct WikiContextProvider {
    name: String,
    wiki_name: String,
    source: PathBuf,
    top_k: usize,
    /// v2: 是否启用混合检索（BM25 + 向量 + 图遍历）。
    hybrid: bool,
    engine: OnceCell<Arc<WikiEngine>>,
}

impl WikiContextProvider {
    pub fn new(name: impl Into<String>, source: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            wiki_name: name.clone(),
            name,
            source: PathBuf::from(source.into()),
            top_k: DEFAULT_TOP_K,
            hybrid: false,
            engine: OnceCell::new(),
        }
    }

    pub fn with_top_k(mut self, k: usize) -> Self {
        self.top_k = k;
        self
    }

    /// v2: 启用混合检索（BM25 + 向量 + 图遍历，RRF 融合）。
    ///
    /// 启用后会在首次挂载时构建向量索引。向量索引使用默认的 Simple 嵌入模型（384 维）。
    pub fn with_hybrid(mut self) -> Self {
        self.hybrid = true;
        self
    }

    async fn engine(&self) -> Result<Arc<WikiEngine>> {
        self.engine
            .get_or_try_init(|| async {
                let wiki_name = self.wiki_name.clone();
                let source = self.source.clone();
                let hybrid = self.hybrid;
                let engine = tokio::task::spawn_blocking(move || {
                    WikiEngine::from_repo(&wiki_name, &source).map(Arc::new)
                })
                .await
                .map_err(|e| {
                    rust_agent_core::AgentError::ConfigError(format!("wiki task: {e}"))
                })?
                .map_err(|e| {
                    rust_agent_core::AgentError::ConfigError(format!("wiki mount: {e}"))
                })?;
                // v2: 启用混合检索时构建向量索引。
                if hybrid {
                    engine.enable_vector_search(&self.wiki_name).map_err(|e| {
                        rust_agent_core::AgentError::ConfigError(format!(
                            "wiki enable vector: {e}"
                        ))
                    })?;
                    engine.build_vector_index(&self.wiki_name).await.map_err(|e| {
                        rust_agent_core::AgentError::ConfigError(format!(
                            "wiki build vector: {e}"
                        ))
                    })?;
                }
                Ok::<_, rust_agent_core::AgentError>(engine)
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
impl IContextProvider for WikiContextProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> &str {
        "wiki"
    }

    async fn enrich_instructions(&self, _ctx: &ProviderContext<'_>) -> Result<Option<String>> {
        Ok(Some(format!(
            "## Wiki: {}\n\
             路径: {}\n\
             与用户问题相关的 wiki 页面会在检索后注入上下文。",
            self.name,
            self.source.display()
        )))
    }

    async fn enrich_messages(&self, ctx: &ProviderContext<'_>) -> Result<MessageInjection> {
        let query = match Self::last_user_query(ctx.messages) {
            Some(q) => q,
            None => return Ok(MessageInjection::default()),
        };

        let engine = self.engine().await?;
        let wiki_name = self.wiki_name.clone();
        let top_k = self.top_k;
        let hybrid = self.hybrid;

        let hits = tokio::task::spawn_blocking(move || {
            let state = engine
                .state
                .read()
                .map_err(|_| rust_agent_core::AgentError::ConfigError("wiki lock poisoned".into()))?;
            wiki_search(
                &state,
                &wiki_name,
                &SearchParams {
                    query: &query,
                    type_filter: None,
                    no_excerpt: false,
                    top_k: Some(top_k),
                    include_sections: false,
                    cross_wiki: false,
                    hybrid,
                    vector_weight: None,
                    graph_weight: None,
                    graph_hops: None,
                },
            )
            .map_err(|e| rust_agent_core::AgentError::ConfigError(format!("wiki search: {e}")))
        })
        .await
        .map_err(|e| rust_agent_core::AgentError::ConfigError(format!("wiki search task: {e}")))??;

        if hits.results.is_empty() {
            return Ok(MessageInjection::default());
        }

        let mut body = String::from("## Wiki 检索结果\n\n");
        for hit in &hits.results {
            body.push_str(&format!("### {} ({})\n", hit.title, hit.uri));
            if let Some(ref excerpt) = hit.excerpt {
                body.push_str(excerpt);
                body.push('\n');
            } else if let Some(ref summary) = hit.summary {
                body.push_str(summary);
                body.push('\n');
            }
            body.push('\n');
        }

        Ok(MessageInjection {
            messages: vec![ChatMessage::system(body)],
            replace: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mounts_flat_markdown_wiki() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("intro.md"),
            "---\ntitle: Intro\n---\n\nRust agent wiki content.",
        )
        .unwrap();

        let provider = WikiContextProvider::new("test-wiki", dir.path().to_string_lossy());
        let engine = provider.engine().await.expect("wiki engine");
        assert!(engine.state.read().unwrap().spaces.contains_key("test-wiki"));
    }
}
