use serde::{Deserialize, Serialize};

// ─── Phase-2 Input Types (non-FromRow) ───────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CreateSearchProviderInput {
    pub name: String,
    pub provider_type: String,
    pub endpoint: Option<String>,
    pub api_key: Option<String>,
    pub enabled: Option<bool>,
    pub region: Option<String>,
    pub language: Option<String>,
    pub safe_search: Option<bool>,
    pub result_limit: Option<i32>,
    pub timeout_ms: Option<i32>,
}
