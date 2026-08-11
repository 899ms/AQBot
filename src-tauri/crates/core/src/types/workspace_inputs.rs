use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateArtifactInput {
    pub conversation_id: String,
    pub source_message_id: Option<String>,
    pub kind: String,
    pub title: String,
    pub content: String,
    pub format: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateArtifactInput {
    pub title: Option<String>,
    pub content: Option<String>,
    pub format: Option<String>,
    pub pinned: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateContextSourceInput {
    pub conversation_id: String,
    pub message_id: Option<String>,
    pub source_type: String,
    pub ref_id: String,
    pub title: String,
    pub summary: Option<String>,
}
