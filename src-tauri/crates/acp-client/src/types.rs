use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpProject {
    pub id: String,
    pub name: String,
    pub root_path: String,
    pub created_at: String,
    pub updated_at: String,
    pub last_opened_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpThread {
    pub id: String,
    pub project_id: String,
    pub agent_id: String,
    pub title: String,
    pub acp_session_id: Option<String>,
    pub runtime_status: String,
    pub mode_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpMessage {
    pub id: String,
    pub thread_id: String,
    pub role: String,
    pub content: String,
    pub status: Option<String>,
    pub attachments_json: Option<String>,
    pub meta_json: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProbeResult {
    pub agent_id: String,
    pub available: bool,
    pub command: String,
    pub message: String,
}
