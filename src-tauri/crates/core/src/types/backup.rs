use serde::{Deserialize, Serialize};

// Backup & Migration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupManifest {
    pub id: String,
    pub version: String,
    pub created_at: String,
    pub encrypted: bool,
    pub checksum: String,
    pub object_counts_json: String,
    pub source_app_version: String,
    pub file_path: Option<String>,
    pub file_size: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupTarget {
    pub id: String,
    pub kind: String, // local | webdav | s3
    pub config_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoBackupSettings {
    pub enabled: bool,
    pub interval_hours: u32,
    pub max_count: u32,
    pub backup_dir: Option<String>,
}
