use serde::{Deserialize, Serialize};

// Memory
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryNamespace {
    pub id: String,
    pub name: String,
    pub scope: String, // global | project
    pub embedding_provider: Option<String>,
    pub embedding_dimensions: Option<i32>,
    pub retrieval_threshold: Option<f32>,
    pub retrieval_top_k: Option<i32>,
    pub icon_type: Option<String>,
    pub icon_value: Option<String>,
    pub sort_order: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryItem {
    pub id: String,
    pub namespace_id: String,
    pub title: String,
    pub content: String,
    pub source: String,       // manual | auto_extract
    pub index_status: String, // pending | indexing | ready | failed | skipped
    pub index_error: Option<String>,
    pub updated_at: String,
}
