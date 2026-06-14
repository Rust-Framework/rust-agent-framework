use serde::{Deserialize, Serialize};

/// Model list entry item from GET /models API.
/// Both OpenAI (`/v1/models`) and DeepSeek (`/models`) follow this format:
/// `{ "object": "list", "data": [{ "id": "...", "object": "model", ... }] }`
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelListEntry {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub owned_by: String,
}
