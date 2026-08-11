use super::serde_helpers::deserialize_double_option;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CreateMcpServerInput {
    pub name: String,
    pub transport: String,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub endpoint: Option<String>,
    pub env: Option<std::collections::HashMap<String, String>>,
    pub enabled: Option<bool>,
    pub permission_policy: Option<String>,
    pub source: Option<String>,
    pub discover_timeout_secs: Option<i32>,
    pub execute_timeout_secs: Option<i32>,
    pub headers_json: Option<String>,
    pub icon_type: Option<String>,
    pub icon_value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateMcpServerInput {
    pub name: Option<String>,
    pub transport: Option<String>,
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub command: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub args: Option<Option<Vec<String>>>,
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub endpoint: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub env: Option<Option<std::collections::HashMap<String, String>>>,
    pub enabled: Option<bool>,
    pub permission_policy: Option<String>,
    pub source: Option<String>,
    pub discover_timeout_secs: Option<i32>,
    pub execute_timeout_secs: Option<i32>,
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub headers_json: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub icon_type: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub icon_value: Option<Option<String>>,
}
