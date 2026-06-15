use async_trait::async_trait;
use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use crate::Result;

/// 向量存储抽象
///
/// 提供向量数据的 upsert/search/delete 操作，
/// 用于实现 RAG（检索增强生成）和记忆系统。
#[async_trait]
pub trait IVectorStore: Send + Sync {
    /// 插入或更新一条向量记录
    async fn upsert(
        &self,
        id: &str,
        embedding: Vec<f32>,
        metadata: HashMap<String, Value>,
    ) -> Result<()>;

    /// 搜索最相似的向量记录
    async fn search(
        &self,
        query_embedding: Vec<f32>,
        top_k: usize,
        filter: Option<HashMap<String, Value>>,
    ) -> Result<Vec<SearchResult>>;

    /// 删除一条向量记录
    async fn delete(&self, id: &str) -> Result<()>;
}

/// 向量搜索结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub id: String,
    pub score: f32,
    pub metadata: HashMap<String, Value>,
}
