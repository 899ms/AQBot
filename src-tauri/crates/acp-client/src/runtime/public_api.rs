/// Stable host-owned status codes. The UI localizes these values; free-form
/// Agent status messages remain untouched.
pub const ACP_STATUS_CANCEL_RESTARTING: &str = "aqbot:cancel-restarting";
pub const ACP_STATUS_USING_SHARED_AGENT: &str = "aqbot:using-shared-agent";
pub const ACP_STATUS_LAUNCHING_AGENT: &str = "aqbot:launching-agent";
pub const ACP_STATUS_AGENT_READY: &str = "aqbot:agent-ready";
pub const ACP_STATUS_RESTORING_SESSION: &str = "aqbot:restoring-session";
pub const ACP_STATUS_SAVED_SESSION_EXPIRED: &str = "aqbot:saved-session-expired";
pub const ACP_STATUS_CREATING_SESSION: &str = "aqbot:creating-session";
pub const ACP_STATUS_SENDING_PROMPT: &str = "aqbot:sending-prompt";
pub const ACP_STATUS_SESSION_EXPIRED: &str = "aqbot:session-expired";
pub const ACP_STATUS_GROK_RETRY_PREFIX: &str = "aqbot:grok-retry:";

/// UI-facing events emitted during a prompt turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AcpEvent {
    #[serde(rename_all = "camelCase")]
    StreamText { text: String },
    #[serde(rename_all = "camelCase")]
    StreamThinking { thinking: String },
    #[serde(rename_all = "camelCase")]
    ToolCall {
        tool_call_id: String,
        title: Option<String>,
        kind: Option<String>,
        status: Option<String>,
        raw: serde_json::Value,
    },
    #[serde(rename_all = "camelCase")]
    ToolCallUpdate {
        tool_call_id: String,
        status: Option<String>,
        raw: serde_json::Value,
    },
    #[serde(rename_all = "camelCase")]
    Plan { raw: serde_json::Value },
    #[serde(rename_all = "camelCase")]
    SessionState { snapshot: AcpSessionSnapshot },
    #[serde(rename_all = "camelCase")]
    PermissionRequest {
        request_id: String,
        interaction_kind: AcpInteractionKind,
        tool_call_id: Option<String>,
        title: Option<String>,
        raw: serde_json::Value,
        options: Vec<PermissionOptionView>,
    },
    #[serde(rename_all = "camelCase")]
    InteractionClosed {
        request_id: String,
        interaction_kind: AcpInteractionKind,
        tool_call_id: Option<String>,
        outcome: AcpInteractionOutcome,
        selected_option_id: Option<String>,
        selected_option_kind: Option<String>,
        selected_option_name: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Status { message: String },
    #[serde(rename_all = "camelCase")]
    Error { message: String },
    #[serde(rename_all = "camelCase")]
    Done {
        stop_reason: String,
        session_id: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AcpInteractionKind {
    Permission,
    Question,
    PlanReview,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AcpInteractionOutcome {
    Selected,
    Cancelled,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionOptionView {
    pub option_id: String,
    pub name: String,
    pub kind: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PromptOutcome {
    pub session_id: String,
    pub stop_reason: String,
    pub snapshot: AcpSessionSnapshot,
}

/// A prompt that has been accepted by the live ACP session worker.
pub struct AcpPromptHandle {
    session_key: String,
    permission_scope: String,
    permissions: PermissionMap,
    sessions: Arc<Mutex<HashMap<String, LiveSession>>>,
    reply_rx: oneshot::Receiver<anyhow::Result<PromptOutcome>>,
}

impl AcpPromptHandle {
    /// Wait for the scheduled prompt turn to finish.
    pub async fn wait(self) -> anyhow::Result<PromptOutcome> {
        let Self {
            session_key,
            permission_scope,
            permissions,
            sessions,
            reply_rx,
        } = self;
        match reply_rx.await {
            Ok(result) => {
                if result.is_err() {
                    remove_session_if_current(&sessions, &session_key, &permission_scope).await;
                    cancel_permission_scope(&permissions, &permission_scope).await;
                }
                result
            }
            Err(_) => {
                remove_session_if_current(&sessions, &session_key, &permission_scope).await;
                cancel_permission_scope(&permissions, &permission_scope).await;
                anyhow::bail!("agent session worker exited")
            }
        }
    }
}

/// User input for one ACP prompt turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpPromptInput {
    pub text: String,
    pub attachments: Vec<AcpPromptAttachment>,
}

/// A persisted local attachment prepared by the application layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpPromptAttachment {
    pub file_name: String,
    pub mime_type: String,
    pub file_size: u64,
    /// Base64 payload. Required for images and unused for resource links.
    pub data: Option<String>,
    /// URI of AQBot's persisted copy of the attachment.
    pub file_uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpSessionSnapshot {
    pub session_id: String,
    pub modes: Option<SessionModeState>,
    pub config_options: Vec<SessionConfigOption>,
    pub agent_capabilities: AgentCapabilities,
}

#[derive(Debug, Clone, Copy)]
pub struct RuntimeLimits {
    pub idle_timeout: Duration,
    /// `0` means unlimited.
    pub max_processes: usize,
    session_control_timeout: Duration,
}

impl RuntimeLimits {
    pub fn new(idle_timeout_secs: u64, max_processes: u32) -> Self {
        Self {
            idle_timeout: Duration::from_secs(idle_timeout_secs),
            max_processes: max_processes as usize,
            session_control_timeout: Duration::from_secs(30),
        }
    }

    #[cfg(test)]
    fn with_session_control_timeout(mut self, timeout: Duration) -> Self {
        self.session_control_timeout = timeout;
        self
    }
}
