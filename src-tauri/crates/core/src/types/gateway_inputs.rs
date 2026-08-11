use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveProgramPolicyInput {
    pub program_name: String,
    pub allowed_provider_ids: Vec<String>,
    pub allowed_model_ids: Vec<String>,
    pub default_provider_id: Option<String>,
    pub default_model_id: Option<String>,
    pub rate_limit_per_minute: Option<i32>,
}
