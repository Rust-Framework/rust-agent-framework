use serde::{Deserialize, Serialize};

/// 来自 GET /models API 的模型列表条目。
/// OpenAI（`/v1/models`）和 DeepSeek（`/models`）均遵循此格式：
/// `{ "object": "list", "data": [{ "id": "...", "object": "model", ... }] }`
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelListEntry {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub owned_by: String,
}
