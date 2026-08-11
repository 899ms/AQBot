use serde::{Deserialize, Serialize};

// Desktop
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopState {
    pub window_key: String, // main | mini | voice | artifact
    pub width: i32,
    pub height: i32,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub maximized: bool,
    pub visible: bool,
}
