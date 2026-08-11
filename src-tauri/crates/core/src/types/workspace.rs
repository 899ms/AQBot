use serde::{Deserialize, Serialize};

// Artifacts
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub id: String,
    pub conversation_id: String,
    pub kind: String, // draft | note | report | snippet | checklist
    pub title: String,
    pub content: String,
    pub format: String, // markdown | text | json
    pub pinned: bool,
    pub updated_at: String,
}

// Context Sources
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextSource {
    pub id: String,
    pub conversation_id: String,
    pub message_id: Option<String>,
    #[serde(rename = "type")]
    pub source_type: String, // app | attachment | search | knowledge | memory | tool
    pub ref_id: String,
    pub title: String,
    pub enabled: bool,
    pub summary: Option<String>,
}

// Conversation Branches
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationBranch {
    pub id: String,
    pub conversation_id: String,
    pub parent_message_id: String,
    pub branch_label: String,
    pub branch_index: i32,
    pub compared_message_ids_json: Option<String>,
    pub created_at: String,
}
