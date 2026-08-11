use serde::{Deserialize, Serialize};

// === Realtime Voice ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealtimeConfig {
    pub model_id: String,
    pub voice: Option<String>,
    pub audio_format: AudioFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub channels: u8,
    pub encoding: AudioEncoding,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AudioEncoding {
    Pcm16,
    Opus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum VoiceSessionState {
    Idle,
    Connecting,
    Connected,
    Speaking,
    Listening,
    Disconnecting,
}
