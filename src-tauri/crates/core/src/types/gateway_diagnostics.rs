use serde::{Deserialize, Serialize};

// Gateway Phase-2
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramPolicy {
    pub id: String,
    pub program_name: String,
    pub allowed_provider_ids_json: String,
    pub allowed_model_ids_json: String,
    pub default_provider_id: Option<String>,
    pub default_model_id: Option<String>,
    pub rate_limit_per_minute: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayDiagnostic {
    pub id: String,
    pub category: String, // provider_latency | provider_error | proxy | auth | port
    pub status: String,   // ok | warning | error
    pub message: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayRequestLog {
    pub id: String,
    pub key_id: String,
    pub key_name: String,
    pub method: String,
    pub path: String,
    pub model: Option<String>,
    pub provider_id: Option<String>,
    pub status_code: i32,
    pub duration_ms: i32,
    pub request_tokens: i32,
    pub response_tokens: i32,
    pub error_message: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayTemplate {
    pub id: String,
    pub name: String,
    pub target: String, // cursor | vscode | claude_code | openai_compatible
    pub format: String, // json | yaml | markdown
    pub content: String,
    pub copy_hint: Option<String>,
}
