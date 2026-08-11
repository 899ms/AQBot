use serde::{Deserialize, Serialize};

// MCP & Tools
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServer {
    pub id: String,
    pub name: String,
    pub transport: String, // stdio | http | sse
    pub command: Option<String>,
    pub args_json: Option<String>,
    pub endpoint: Option<String>,
    pub env_json: Option<String>,
    pub enabled: bool,
    pub permission_policy: String, // ask | allow_safe | allow_all
    pub source: String,            // builtin | custom
    pub discover_timeout_secs: Option<i32>,
    pub execute_timeout_secs: Option<i32>,
    pub headers_json: Option<String>,
    pub icon_type: Option<String>,
    pub icon_value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDescriptor {
    pub id: String,
    pub server_id: String,
    pub name: String,
    pub description: Option<String>,
    pub input_schema_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolExecution {
    pub id: String,
    pub conversation_id: String,
    pub message_id: Option<String>,
    pub server_id: String,
    pub tool_name: String,
    pub status: String, // pending | running | success | failed | cancelled
    pub input_preview: Option<String>,
    pub output_preview: Option<String>,
    pub error_message: Option<String>,
    pub duration_ms: Option<i64>,
    pub created_at: String,
    pub approval_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSession {
    pub id: String,
    pub conversation_id: String,
    pub cwd: Option<String>,
    pub permission_mode: String,
    pub runtime_status: String,
    pub sdk_context_json: Option<String>,
    pub sdk_context_backup_json: Option<String>,
    pub total_tokens: i32,
    pub total_cost_usd: f64,
    pub created_at: String,
    pub updated_at: String,
}
