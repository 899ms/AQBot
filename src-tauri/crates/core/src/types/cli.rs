use serde::{Deserialize, Serialize};

// CLI Tool Integration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliToolInfo {
    pub id: String,
    pub name: String,
    pub status: String, // not_installed | not_connected | connected
    pub version: Option<String>,
    pub config_path: Option<String>,
    pub has_backup: bool,
    pub connected_protocol: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSessionVisibilityRepairResult {
    pub target_provider: String,
    pub changed_session_files: usize,
    pub skipped_locked_session_files: usize,
    pub sqlite_rows_updated: usize,
    pub sqlite_provider_rows_updated: usize,
    pub sqlite_user_event_rows_updated: usize,
    pub sqlite_cwd_rows_updated: usize,
    pub sqlite_present: bool,
    pub updated_workspace_roots: usize,
    pub backup_dir: Option<String>,
    pub encrypted_content_warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSessionVisibilityStatusRow {
    pub scope: String,
    pub provider: Option<String>,
    pub count: usize,
    pub mismatched_count: usize,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSessionVisibilityStatus {
    pub target_provider: String,
    pub codex_home: String,
    pub total_session_files: usize,
    pub mismatched_session_files: usize,
    pub sqlite_present: bool,
    pub sqlite_rows: usize,
    pub sqlite_mismatched_rows: usize,
    pub sqlite_user_event_rows_needing_repair: usize,
    pub sqlite_cwd_rows_needing_repair: usize,
    pub workspace_roots_needing_update: usize,
    pub status_rows: Vec<CodexSessionVisibilityStatusRow>,
    pub encrypted_content_warning: Option<String>,
}
