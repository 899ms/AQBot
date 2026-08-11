use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateKnowledgeBaseInput {
    pub name: String,
    pub description: Option<String>,
    pub embedding_provider: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateKnowledgeBaseInput {
    pub name: Option<String>,
    pub description: Option<String>,
    pub embedding_provider: Option<String>,
    pub enabled: Option<bool>,
    pub icon_type: Option<String>,
    pub icon_value: Option<String>,
    #[serde(default)]
    pub update_icon: bool,
    pub embedding_dimensions: Option<i32>,
    #[serde(default)]
    pub update_embedding_dimensions: bool,
    pub retrieval_threshold: Option<f32>,
    #[serde(default)]
    pub update_retrieval_threshold: bool,
    pub retrieval_top_k: Option<i32>,
    #[serde(default)]
    pub update_retrieval_top_k: bool,
    pub rerank_provider: Option<String>,
    #[serde(default)]
    pub update_rerank_provider: bool,
    pub rerank_candidate_k: Option<i32>,
    #[serde(default)]
    pub update_rerank_candidate_k: bool,
    pub chunk_size: Option<i32>,
    #[serde(default)]
    pub update_chunk_size: bool,
    pub chunk_overlap: Option<i32>,
    #[serde(default)]
    pub update_chunk_overlap: bool,
    pub separator: Option<String>,
    #[serde(default)]
    pub update_separator: bool,
    pub index_concurrency: Option<i32>,
    #[serde(default)]
    pub update_index_concurrency: bool,
    pub index_interval_ms: Option<i32>,
    #[serde(default)]
    pub update_index_interval_ms: bool,
}
