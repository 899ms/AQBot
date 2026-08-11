use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBackupJobInput {
    pub target_kind: String,
    pub target_config_json: String,
    pub include_attachments: bool,
    pub include_knowledge_files: bool,
    pub include_gateway_config: bool,
    pub passphrase: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSourceInput {
    pub source_type: String,
    pub path: String,
    pub credentials_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPolicyInput {
    pub duplicate_strategy: String, // skip | rename | overwrite
    pub merge_settings: bool,
    pub merge_apps: bool,
    pub dry_run: bool,
}
