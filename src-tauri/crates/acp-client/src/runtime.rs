//! ACP runtime: spawn external agents and run prompt turns.
//!
//! Live agent processes are kept per `session_key` (AQBot thread id) so multi-turn
//! prompts reuse the same process. After process death / app restart we try
//! `session/load`, then fall back to `session/new` — never prompt with a bare
//! stale session id (that caused "Session … not found").

use crate::config::ConfiguredAgent;
use agent_client_protocol::schema::v1::{
    AgentCapabilities, AgentNotification, BooleanConfigOptionCapabilities, CancelNotification,
    ClientCapabilities, ClientNotification, ClientSessionCapabilities, ContentBlock,
    ExtNotification, ImageContent, InitializeRequest, LoadSessionRequest, McpServer,
    NewSessionResponse, PermissionOptionKind, PromptRequest, RequestPermissionOutcome,
    RequestPermissionRequest, RequestPermissionResponse, ResourceLink, ResumeSessionRequest,
    SelectedPermissionOutcome, SessionConfigKind, SessionConfigOption, SessionConfigOptionCategory,
    SessionConfigOptionValue, SessionConfigOptionsCapabilities, SessionConfigSelectOption,
    SessionConfigSelectOptions, SessionId, SessionMode, SessionModeId, SessionModeState,
    SessionNotification, SessionUpdate, SetSessionConfigOptionRequest, SetSessionModeRequest,
    TextContent,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::{
    AcpAgent, AcpAgentConfig, Agent, ConnectionTo, JsonRpcRequest, JsonRpcResponse, Responder,
};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering},
    Arc, Mutex as StdMutex, OnceLock,
};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot, watch, Mutex};

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
}

impl RuntimeLimits {
    pub fn new(idle_timeout_secs: u64, max_processes: u32) -> Self {
        Self {
            idle_timeout: Duration::from_secs(idle_timeout_secs),
            max_processes: max_processes as usize,
        }
    }
}

#[derive(Debug, Clone)]
struct PermissionResolution {
    option_id: String,
    feedback: Option<String>,
}

struct PendingPermission {
    scope: String,
    interaction_kind: AcpInteractionKind,
    tool_call_id: Option<String>,
    options: Vec<PermissionOptionView>,
    questionnaire: Option<GrokQuestionnaireContext>,
    event_tx: mpsc::UnboundedSender<AcpEvent>,
    sender: Option<oneshot::Sender<PermissionResolution>>,
    questionnaire_sender: Option<oneshot::Sender<AcpQuestionnaireSubmission>>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AcpQuestionnaireOutcome {
    Accepted,
    ChatAboutThis,
    SkipInterview,
    Cancelled,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AcpQuestionnaireAnswer {
    pub question_index: usize,
    #[serde(default)]
    pub selected_option_indexes: Vec<usize>,
    #[serde(default)]
    pub other_text: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AcpQuestionnaireSubmission {
    pub outcome: AcpQuestionnaireOutcome,
    #[serde(default)]
    pub answers: Vec<AcpQuestionnaireAnswer>,
}

type PermissionMap = Arc<Mutex<HashMap<String, PendingPermission>>>;
type EventTxSlot = Arc<Mutex<Option<mpsc::UnboundedSender<AcpEvent>>>>;
type ConnectionSlot = Arc<Mutex<Option<ConnectionTo<Agent>>>>;
type RouteMap = Arc<Mutex<SessionRoutes>>;

fn emit_interaction_closed(
    event_tx: &mpsc::UnboundedSender<AcpEvent>,
    request_id: &str,
    interaction_kind: AcpInteractionKind,
    tool_call_id: Option<String>,
    outcome: AcpInteractionOutcome,
    selected: Option<&PermissionOptionView>,
) {
    if let Err(error) = event_tx.send(AcpEvent::InteractionClosed {
        request_id: request_id.to_string(),
        interaction_kind,
        tool_call_id,
        outcome,
        selected_option_id: selected.map(|option| option.option_id.clone()),
        selected_option_kind: selected.and_then(|option| option.kind.clone()),
        selected_option_name: selected.map(|option| option.name.clone()),
    }) {
        tracing::warn!(%error, request_id, "failed to emit ACP interaction terminal event");
    }
}

async fn expire_permission(permissions: &PermissionMap, request_id: &str) {
    let pending = permissions.lock().await.remove(request_id);
    if let Some(pending) = pending {
        emit_interaction_closed(
            &pending.event_tx,
            request_id,
            pending.interaction_kind,
            pending.tool_call_id.clone(),
            AcpInteractionOutcome::Expired,
            None,
        );
    }
}

async fn cancel_permission_scope(permissions: &PermissionMap, scope: &str) {
    let mut permissions = permissions.lock().await;
    let request_ids = permissions
        .iter()
        .filter(|(_, pending)| pending.scope == scope)
        .map(|(request_id, _)| request_id.clone())
        .collect::<Vec<_>>();
    let cancelled = request_ids
        .into_iter()
        .filter_map(|request_id| {
            permissions
                .remove(&request_id)
                .map(|pending| (request_id, pending))
        })
        .collect::<Vec<_>>();
    drop(permissions);
    for (request_id, pending) in cancelled {
        emit_interaction_closed(
            &pending.event_tx,
            &request_id,
            pending.interaction_kind,
            pending.tool_call_id.clone(),
            AcpInteractionOutcome::Cancelled,
            None,
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LaunchFingerprint {
    agent_id: String,
    command: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
    /// Grok's permission extension is process-scoped, so differently trusted
    /// conversations must not share its transport.
    grok_auto_approve: Option<bool>,
}

impl LaunchFingerprint {
    fn new(agent: &ConfiguredAgent, auto_approve: bool) -> Self {
        let mut env = agent
            .env
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<Vec<_>>();
        env.sort_unstable();
        Self {
            agent_id: agent.id.clone(),
            command: agent.command.clone(),
            args: agent.args.clone(),
            env,
            grok_auto_approve: is_grok_launch(agent).then_some(auto_approve),
        }
    }

    fn matches_agent(&self, agent: &ConfiguredAgent) -> bool {
        self.agent_id == agent.id
            && self.command == agent.command
            && self.args == agent.args
            && self.env == {
                let mut env = agent
                    .env
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect::<Vec<_>>();
                env.sort_unstable();
                env
            }
    }
}

fn is_grok_launch(agent: &ConfiguredAgent) -> bool {
    [&agent.id, &agent.name, &agent.command]
        .into_iter()
        .any(|value| value.to_ascii_lowercase().contains("grok"))
}

#[derive(Clone)]
struct SessionRoute {
    active: Arc<Mutex<ActiveSession>>,
    event_slot: EventTxSlot,
    auto_approve: Arc<AtomicBool>,
    prompt_state: Arc<AtomicU8>,
    prompt_dispatch_lock: Arc<Mutex<()>>,
    permission_scope: String,
}

#[derive(Default)]
struct SessionRoutes {
    by_session_id: HashMap<String, SessionRoute>,
    opening: Option<SessionRoute>,
}

#[derive(Debug, Clone)]
struct AgentMetadata {
    capabilities: AgentCapabilities,
    meta: Option<agent_client_protocol::schema::v1::Meta>,
    launch_config_options: Vec<SessionConfigOption>,
}

#[derive(Debug, Clone)]
struct LaunchOptionCatalog {
    models: Vec<String>,
    reasoning_efforts: Vec<String>,
}

static LAUNCH_OPTION_CACHE: OnceLock<Mutex<HashMap<String, LaunchOptionCatalog>>> = OnceLock::new();

const GROK_PERMISSION_CONFIG_ID: &str = "aqbot_grok_permission";
const PROMPT_IDLE: u8 = 0;
const PROMPT_QUEUED: u8 = 1;
const PROMPT_RUNNING: u8 = 2;
const PROMPT_CANCEL_REQUESTED: u8 = 3;
const RUNNING_CANCEL_GRACE: Duration = Duration::from_secs(2);
const PROCESS_SHUTDOWN_GRACE: Duration = Duration::from_secs(1);
// ACP extension methods are sent on the wire with a leading underscore. The
// protocol dispatcher removes it before Grok's `ext_notification` handler sees
// `x.ai/yolo_mode_changed`.
const GROK_PERMISSION_SET_METHOD: &str = "_x.ai/yolo_mode_changed";
const PERSISTED_CONFIG_MODE_PREFIX: &str = "aqbot-config-mode:";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedConfigMode {
    config_id: String,
    value: String,
}

#[derive(Debug, Default)]
struct ActiveSession {
    id: Option<SessionId>,
    modes: Option<SessionModeState>,
    config_options: Vec<SessionConfigOption>,
}

#[derive(Debug, Clone)]
enum ReadyState {
    Starting,
    Ready,
    Failed(String),
}

struct PromptJob {
    cwd: PathBuf,
    prompt: Vec<ContentBlock>,
    preferred_session_id: Option<String>,
    event_tx: mpsc::UnboundedSender<AcpEvent>,
    generation: u64,
    reply: oneshot::Sender<anyhow::Result<PromptOutcome>>,
}

enum NotificationWork {
    Session(SessionNotification),
    Extension(ExtNotification),
    Barrier(oneshot::Sender<()>),
}

async fn drain_notification_work(
    notification_tx: &mpsc::UnboundedSender<NotificationWork>,
) -> anyhow::Result<()> {
    let (drained_tx, drained_rx) = oneshot::channel();
    notification_tx
        .send(NotificationWork::Barrier(drained_tx))
        .map_err(|_| anyhow::anyhow!("ACP notification worker exited"))?;
    drained_rx
        .await
        .map_err(|_| anyhow::anyhow!("ACP notification drain failed"))
}

struct BusyGuard(Arc<AtomicUsize>);

impl BusyGuard {
    fn activate(flag: Arc<AtomicUsize>) -> Self {
        flag.fetch_add(1, Ordering::AcqRel);
        Self(flag)
    }
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        let previous = self.0.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous > 0, "ACP busy guard counter underflow");
    }
}

#[derive(Clone)]
struct LiveSession {
    job_tx: mpsc::UnboundedSender<PromptJob>,
    /// Keeps the process owner's receive loop alive while any logical session
    /// still references this transport.
    process_keepalive: mpsc::UnboundedSender<PromptJob>,
    fingerprint: LaunchFingerprint,
    process_scope: String,
    agent_id: String,
    configured_agent: ConfiguredAgent,
    cwd: PathBuf,
    ready: watch::Receiver<ReadyState>,
    discovery_ready: watch::Receiver<bool>,
    connection: ConnectionSlot,
    metadata: Arc<Mutex<Option<AgentMetadata>>>,
    routes: RouteMap,
    notification_barrier_tx: mpsc::UnboundedSender<NotificationWork>,
    session_open_lock: Arc<Mutex<()>>,
    process_operation_lock: Arc<Mutex<()>>,
    event_slot: EventTxSlot,
    active: Arc<Mutex<ActiveSession>>,
    admission_lock: Arc<Mutex<()>>,
    operation_lock: Arc<Mutex<()>>,
    auto_approve: Arc<AtomicBool>,
    busy: Arc<AtomicUsize>,
    prompt_state: Arc<AtomicU8>,
    prompt_dispatch_lock: Arc<Mutex<()>>,
    prompt_generation: Arc<AtomicU64>,
    completed_generation: Arc<AtomicU64>,
    completion_tx: watch::Sender<u64>,
    cancel_tx: watch::Sender<u64>,
    process_shutdown: Arc<AtomicBool>,
    process_abort: Arc<tokio::task::AbortHandle>,
    runtime_limits: Arc<StdMutex<RuntimeLimits>>,
    last_used: Arc<StdMutex<Instant>>,
    process_last_used: Arc<StdMutex<Instant>>,
    permission_scope: String,
}

/// Shared runtime handle for the app.
pub struct AcpRuntime {
    permissions: PermissionMap,
    sessions: Arc<Mutex<HashMap<String, LiveSession>>>,
    /// Process anchors keyed by immutable launch settings. Anchors are never
    /// claimed by a thread; logical sessions fork from them and share transport.
    warm_sessions: Mutex<HashMap<LaunchFingerprint, LiveSession>>,
    pool_lock: Mutex<()>,
    session_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    process_reservations: StdMutex<HashSet<String>>,
    retiring_processes: StdMutex<HashSet<String>>,
}

pub struct CapabilityDiscoveryHandle {
    live: LiveSession,
    sessions: Arc<Mutex<HashMap<String, LiveSession>>>,
}

impl CapabilityDiscoveryHandle {
    pub async fn wait(self) -> anyhow::Result<Option<(String, AcpSessionSnapshot)>> {
        let mut ready = self.live.discovery_ready.clone();
        while !*ready.borrow() {
            ready
                .changed()
                .await
                .map_err(|_| anyhow::anyhow!("ACP capability discovery task exited"))?;
        }
        let metadata = live_metadata(&self.live).await?;
        let snapshot = {
            let active = self.live.active.lock().await;
            snapshot_from_state(&active, &metadata)
        };
        let current_key = self
            .sessions
            .lock()
            .await
            .iter()
            .find(|(_, candidate)| candidate.permission_scope == self.live.permission_scope)
            .map(|(key, _)| key.clone());
        Ok(current_key.map(|key| (key, snapshot)))
    }
}

impl Default for AcpRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl AcpRuntime {
    pub fn new() -> Self {
        Self {
            permissions: Arc::new(Mutex::new(HashMap::new())),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            warm_sessions: Mutex::new(HashMap::new()),
            pool_lock: Mutex::new(()),
            session_locks: Mutex::new(HashMap::new()),
            process_reservations: StdMutex::new(HashSet::new()),
            retiring_processes: StdMutex::new(HashSet::new()),
        }
    }

    /// Keep one initialized process ready for this immutable launch fingerprint.
    /// Threads attach independent ACP sessions without claiming the process.
    pub async fn prewarm_agent(
        &self,
        agent: &ConfiguredAgent,
        auto_approve: bool,
        limits: RuntimeLimits,
    ) -> anyhow::Result<bool> {
        let pool_guard = self.pool_lock.lock().await;
        let fingerprint = LaunchFingerprint::new(agent, auto_approve);
        let sessions = self.sessions.lock().await;
        let mut warm = self.warm_sessions.lock().await;
        if warm
            .get(&fingerprint)
            .is_some_and(|live| !live.process_is_healthy())
        {
            warm.remove(&fingerprint);
        }
        let retiring = self
            .retiring_processes
            .lock()
            .expect("ACP retiring processes lock is poisoned")
            .clone();
        let existing = warm
            .get(&fingerprint)
            .filter(|live| !retiring.contains(&live.process_scope))
            .cloned()
            .or_else(|| {
                sessions
                    .values()
                    .find(|live| {
                        live.fingerprint == fingerprint
                            && live.process_is_healthy()
                            && !retiring.contains(&live.process_scope)
                    })
                    .cloned()
            });
        if let Some(existing) = existing {
            let ready = existing.ready.clone();
            drop(warm);
            drop(sessions);
            drop(pool_guard);
            wait_until_ready(ready).await?;
            return Ok(false);
        }
        warm.remove(&fingerprint);
        if limits.max_processes > 0 && warm.len() >= limits.max_processes {
            drop(warm);
            drop(sessions);
            drop(pool_guard);
            anyhow::bail!(
                "maximum concurrent ACP processes reached ({})",
                limits.max_processes
            );
        }

        let live = spawn_process_anchor(agent, auto_approve, limits, self.permissions.clone())?;
        let ready = live.ready.clone();
        let process_scope = live.process_scope.clone();
        warm.insert(fingerprint.clone(), live);
        drop(warm);
        drop(sessions);
        drop(pool_guard);
        if let Err(error) = wait_until_ready(ready).await {
            let _pool = self.pool_lock.lock().await;
            let mut sessions = self.sessions.lock().await;
            let mut warm = self.warm_sessions.lock().await;
            let removed = remove_process_scope(&mut sessions, &mut warm, &process_scope);
            drop(warm);
            drop(sessions);
            drop(_pool);
            for live in removed {
                unregister_live_route(&live).await;
                self.cancel_permissions(&live.permission_scope).await;
            }
            return Err(error);
        }
        Ok(true)
    }

    pub async fn retain_warm_agents(&self, agents: &[ConfiguredAgent], max_processes: usize) {
        let pool = self.pool_lock.lock().await;
        let sessions = self.sessions.lock().await;
        let mut in_use = sessions
            .values()
            .map(|live| live.process_scope.clone())
            .collect::<HashSet<_>>();
        in_use.extend(
            self.process_reservations
                .lock()
                .expect("ACP process reservations lock is poisoned")
                .iter()
                .cloned(),
        );
        let mut warm = self.warm_sessions.lock().await;
        warm.retain(|fingerprint, live| {
            in_use.contains(&live.process_scope)
                || agents.iter().any(|agent| fingerprint.matches_agent(agent))
        });
        if max_processes > 0 {
            while warm.len() > max_processes {
                let candidate = warm
                    .iter()
                    .filter(|(_, live)| !in_use.contains(&live.process_scope))
                    .max_by_key(|(fingerprint, live)| {
                        (
                            agents
                                .iter()
                                .find(|agent| agent.id == fingerprint.agent_id)
                                .map(|agent| agent.sort)
                                .unwrap_or(i32::MAX),
                            live.process_idle_for(),
                        )
                    })
                    .map(|(fingerprint, _)| fingerprint.clone());
                let Some(candidate) = candidate else {
                    break;
                };
                warm.remove(&candidate);
            }
        }
        drop(warm);
        drop(sessions);
        drop(pool);
    }

    pub async fn resolve_permission(
        &self,
        request_id: &str,
        option_id: String,
        feedback: Option<String>,
    ) -> bool {
        let (pending, selected) = {
            let mut map = self.permissions.lock().await;
            let Some(pending) = map.get_mut(request_id) else {
                return false;
            };
            let Some(selected) = pending
                .options
                .iter()
                .find(|option| option.option_id == option_id)
                .cloned()
            else {
                return false;
            };
            let Some(sender) = pending.sender.take() else {
                return false;
            };
            let trimmed_feedback = feedback
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            if sender
                .send(PermissionResolution {
                    option_id: option_id.clone(),
                    feedback: trimmed_feedback,
                })
                .is_err()
            {
                return false;
            }
            (
                map.remove(request_id)
                    .expect("resolved permission remains registered"),
                selected,
            )
        };
        let PendingPermission {
            interaction_kind,
            tool_call_id,
            event_tx,
            ..
        } = pending;
        emit_interaction_closed(
            &event_tx,
            request_id,
            interaction_kind,
            tool_call_id,
            AcpInteractionOutcome::Selected,
            Some(&selected),
        );
        true
    }

    pub async fn resolve_questionnaire(
        &self,
        request_id: &str,
        submission: AcpQuestionnaireSubmission,
    ) -> Result<String, String> {
        let (pending, summary, outcome) = {
            let mut map = self.permissions.lock().await;
            let pending = map
                .get_mut(request_id)
                .ok_or_else(|| "questionnaire not found or already resolved".to_string())?;
            let context = pending
                .questionnaire
                .as_ref()
                .ok_or_else(|| "interaction is not a questionnaire".to_string())?;
            let summary = validate_questionnaire_submission(context, &submission)?;
            let outcome = submission.outcome;
            let sender = pending
                .questionnaire_sender
                .take()
                .ok_or_else(|| "questionnaire was already resolved".to_string())?;
            sender
                .send(submission)
                .map_err(|_| "questionnaire responder is no longer available".to_string())?;
            let pending = map
                .remove(request_id)
                .expect("resolved questionnaire remains registered");
            (pending, summary, outcome)
        };
        let terminal_outcome = if outcome == AcpQuestionnaireOutcome::Cancelled {
            AcpInteractionOutcome::Cancelled
        } else {
            AcpInteractionOutcome::Selected
        };
        let option_id = match outcome {
            AcpQuestionnaireOutcome::Accepted => "accepted",
            AcpQuestionnaireOutcome::ChatAboutThis => "chat_about_this",
            AcpQuestionnaireOutcome::SkipInterview => "skip_interview",
            AcpQuestionnaireOutcome::Cancelled => "cancelled",
        };
        let selected =
            (outcome != AcpQuestionnaireOutcome::Cancelled).then(|| PermissionOptionView {
                option_id: option_id.into(),
                name: summary.clone(),
                kind: None,
                description: None,
            });
        emit_interaction_closed(
            &pending.event_tx,
            request_id,
            pending.interaction_kind,
            pending.tool_call_id,
            terminal_outcome,
            selected.as_ref(),
        );
        Ok(summary)
    }

    /// Drop a live agent process (e.g. thread deleted).
    pub async fn drop_session(&self, session_key: &str) {
        let removed = self.sessions.lock().await.remove(session_key);
        if let Some(live) = removed {
            unregister_live_route(&live).await;
            self.cancel_permissions(&live.permission_scope).await;
        }
    }

    pub async fn drop_agent_sessions(&self, agent_ids: &[String]) {
        let targets = agent_ids.iter().cloned().collect::<HashSet<_>>();
        if targets.is_empty() {
            return;
        }
        let _pool = self.pool_lock.lock().await;
        let mut sessions = self.sessions.lock().await;
        let keys = sessions
            .iter()
            .filter(|(_, live)| targets.contains(&live.agent_id) && !live.is_active())
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        let removed = keys
            .into_iter()
            .filter_map(|key| sessions.remove(&key))
            .collect::<Vec<_>>();
        let mut in_use = sessions
            .values()
            .map(|live| live.process_scope.clone())
            .collect::<HashSet<_>>();
        in_use.extend(
            self.process_reservations
                .lock()
                .expect("ACP process reservations lock is poisoned")
                .iter()
                .cloned(),
        );
        self.warm_sessions.lock().await.retain(|fingerprint, live| {
            !targets.contains(&fingerprint.agent_id) || in_use.contains(&live.process_scope)
        });
        drop(sessions);
        for live in removed {
            unregister_live_route(&live).await;
            self.cancel_permissions(&live.permission_scope).await;
        }
    }

    pub async fn has_live_session(&self, session_key: &str) -> bool {
        self.sessions.lock().await.contains_key(session_key)
    }

    /// Move a prepared draft process onto its persisted thread key.
    pub async fn adopt_session(&self, from_key: &str, to_key: &str) -> bool {
        if from_key == to_key {
            return self.sessions.lock().await.contains_key(to_key);
        }
        let mut sessions = self.sessions.lock().await;
        if sessions.contains_key(to_key) {
            let removed = sessions.remove(from_key);
            drop(sessions);
            if let Some(live) = removed {
                unregister_live_route(&live).await;
                self.cancel_permissions(&live.permission_scope).await;
            }
            return true;
        }
        let Some(live) = sessions.remove(from_key) else {
            return false;
        };
        live.touch();
        sessions.insert(to_key.to_string(), live);
        true
    }

    /// Read the current normalized state without changing or re-preparing it.
    /// Used when a prepared draft is promoted to a persisted conversation.
    pub async fn session_snapshot(
        &self,
        session_key: &str,
    ) -> anyhow::Result<Option<AcpSessionSnapshot>> {
        let live = self.sessions.lock().await.get(session_key).cloned();
        let Some(live) = live else {
            return Ok(None);
        };
        wait_until_ready(live.ready.clone()).await?;
        let metadata = live_metadata(&live).await?;
        let active = live.active.lock().await;
        Ok(Some(snapshot_from_state(&active, &metadata)))
    }

    /// Wait for optional capability discovery (for example Copilot's model
    /// catalog) and resolve the session's current key after a possible draft
    /// adoption.
    pub async fn wait_for_capability_discovery(
        &self,
        session_key: &str,
    ) -> anyhow::Result<Option<(String, AcpSessionSnapshot)>> {
        let Some(handle) = self.capability_discovery_handle(session_key).await else {
            return Ok(None);
        };
        handle.wait().await
    }

    pub async fn capability_discovery_handle(
        &self,
        session_key: &str,
    ) -> Option<CapabilityDiscoveryHandle> {
        let live = self.sessions.lock().await.get(session_key).cloned()?;
        Some(CapabilityDiscoveryHandle {
            live,
            sessions: self.sessions.clone(),
        })
    }

    /// Restore either a standard session mode or a config-option backed plan
    /// selection persisted by [`persisted_mode_id`]. `None` means the saved
    /// value is no longer advertised by this Agent.
    pub async fn restore_persisted_mode(
        &self,
        session_key: &str,
        persisted: &str,
    ) -> anyhow::Result<Option<AcpSessionSnapshot>> {
        let snapshot = self
            .session_snapshot(session_key)
            .await?
            .ok_or_else(|| anyhow::anyhow!("ACP session process is not running"))?;
        if let Some(encoded) = persisted.strip_prefix(PERSISTED_CONFIG_MODE_PREFIX) {
            let saved: PersistedConfigMode = match serde_json::from_str(encoded) {
                Ok(saved) => saved,
                Err(error) => {
                    tracing::warn!(%error, persisted, "ignoring malformed persisted ACP config mode");
                    return Ok(None);
                }
            };
            return self
                .restore_config_mode(session_key, snapshot, &saved.config_id, &saved.value)
                .await;
        }
        if snapshot.modes.as_ref().is_some_and(|modes| {
            modes
                .available_modes
                .iter()
                .any(|mode| mode.id.to_string() == persisted)
        }) {
            if snapshot
                .modes
                .as_ref()
                .is_some_and(|modes| modes.current_mode_id.to_string() == persisted)
            {
                return Ok(Some(snapshot));
            }
            return self.set_mode(session_key, persisted).await.map(Some);
        }
        // Backward compatibility for rows that stored a config-backed plan as
        // a raw value before the typed encoding was introduced.
        if let Some(option) = snapshot.config_options.iter().find(|option| {
            config_option_contains_plan(option) && config_option_contains_value(option, persisted)
        }) {
            let config_id = option.id.to_string();
            return self
                .restore_config_mode(session_key, snapshot, &config_id, persisted)
                .await;
        }
        Ok(None)
    }

    async fn restore_config_mode(
        &self,
        session_key: &str,
        snapshot: AcpSessionSnapshot,
        config_id: &str,
        value: &str,
    ) -> anyhow::Result<Option<AcpSessionSnapshot>> {
        let Some(option) = snapshot.config_options.iter().find(|option| {
            option.id.to_string() == config_id
                && config_option_contains_plan(option)
                && config_option_contains_value(option, value)
        }) else {
            return Ok(None);
        };
        if current_select_value(option).as_deref() == Some(value) {
            return Ok(Some(snapshot));
        }
        Box::pin(self.set_config_option(session_key, config_id, serde_json::json!(value)))
            .await
            .map(Some)
    }

    /// Start the process and create/resume the ACP session before the user sends.
    pub async fn prepare(
        &self,
        session_key: &str,
        agent: &ConfiguredAgent,
        cwd: PathBuf,
        preferred_session_id: Option<String>,
        auto_approve: bool,
        limits: RuntimeLimits,
        event_tx: mpsc::UnboundedSender<AcpEvent>,
    ) -> anyhow::Result<AcpSessionSnapshot> {
        self.ensure_live(session_key, agent, cwd, auto_approve, limits, &event_tx)
            .await?;
        let live = self.live_session(session_key).await?;
        let _operation = live.operation_lock.lock().await;
        let _busy = BusyGuard::activate(live.busy.clone());
        *live.event_slot.lock().await = Some(event_tx.clone());
        let result = prepare_live_session(&live, preferred_session_id.as_deref(), &event_tx).await;
        let drain_result = drain_notification_work(&live.notification_barrier_tx).await;
        *live.event_slot.lock().await = None;
        live.touch();
        match (result, drain_result) {
            (Ok(snapshot), Ok(())) => Ok(snapshot),
            (Err(error), Ok(())) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Err(error), Err(drain_error)) => Err(anyhow::anyhow!(
                "{error}; ACP notification drain also failed: {drain_error}"
            )),
        }
    }

    pub async fn cancel(&self, session_key: &str) -> anyhow::Result<bool> {
        let live = match self.sessions.lock().await.get(session_key).cloned() {
            Some(live) => live,
            None => return Ok(false),
        };
        let mut cancel_delivery_error = None;
        let generation = {
            let _dispatch = live.prompt_dispatch_lock.lock().await;
            let generation = live.prompt_generation.load(Ordering::Acquire);
            match live.prompt_state.load(Ordering::Acquire) {
                PROMPT_IDLE => return Ok(false),
                PROMPT_CANCEL_REQUESTED => {}
                PROMPT_QUEUED => {
                    live.prompt_state
                        .store(PROMPT_CANCEL_REQUESTED, Ordering::Release);
                    live.cancel_tx.send_replace(generation);
                }
                PROMPT_RUNNING => {
                    live.prompt_state
                        .store(PROMPT_CANCEL_REQUESTED, Ordering::Release);
                    let send_result =
                        async {
                            let session_id =
                                live.active.lock().await.id.clone().ok_or_else(|| {
                                    anyhow::anyhow!("ACP session is not prepared")
                                })?;
                            let connection =
                                live.connection.lock().await.clone().ok_or_else(|| {
                                    anyhow::anyhow!("ACP connection is not ready")
                                })?;
                            connection
                                .send_notification(CancelNotification::new(session_id))
                                .map_err(|e| anyhow::anyhow!("session/cancel failed: {e}"))
                        }
                        .await;
                    if let Err(error) = send_result {
                        cancel_delivery_error = Some(error);
                    }
                }
                state => anyhow::bail!("invalid ACP prompt state `{state}`"),
            }
            generation
        };
        if let Some(error) = cancel_delivery_error.as_ref() {
            tracing::warn!(
                %error,
                process_scope = %live.process_scope,
                "ACP cancel delivery failed; restarting the affected agent process"
            );
            if let Some(event_tx) = live.event_slot.lock().await.clone() {
                let _ = event_tx.send(AcpEvent::Status {
                    message: ACP_STATUS_CANCEL_RESTARTING.into(),
                });
            }
        }
        self.cancel_permissions(&live.permission_scope).await;
        live.touch();

        if cancel_delivery_error.is_some() {
            self.shutdown_process_scope(&live).await;
            let _ = wait_for_prompt_completion(&live, generation, PROCESS_SHUTDOWN_GRACE).await;
            return Ok(true);
        }

        if wait_for_prompt_completion(&live, generation, RUNNING_CANCEL_GRACE).await {
            return Ok(true);
        }
        let _dispatch = live.prompt_dispatch_lock.lock().await;
        if live.completed_generation.load(Ordering::Acquire) >= generation
            || live.prompt_generation.load(Ordering::Acquire) != generation
            || live.prompt_state.load(Ordering::Acquire) != PROMPT_CANCEL_REQUESTED
        {
            return Ok(true);
        }
        self.shutdown_process_scope(&live).await;
        drop(_dispatch);
        let _ = wait_for_prompt_completion(&live, generation, PROCESS_SHUTDOWN_GRACE).await;
        Ok(true)
    }

    pub async fn set_config_option(
        &self,
        session_key: &str,
        config_id: &str,
        value: serde_json::Value,
    ) -> anyhow::Result<AcpSessionSnapshot> {
        let live = self.live_session(session_key).await?;
        let admission = live.admission_lock.lock().await;
        if live.prompt_state.load(Ordering::Acquire) != PROMPT_IDLE {
            anyhow::bail!("cannot change ACP session configuration while a prompt is running");
        }
        let busy_guard = BusyGuard::activate(live.busy.clone());
        if !self
            .sessions
            .lock()
            .await
            .get(session_key)
            .is_some_and(|current| current.permission_scope == live.permission_scope)
        {
            anyhow::bail!("ACP session was replaced before the configuration update started");
        }
        let operation = live.operation_lock.lock().await;
        let connection = live_connection(&live).await?;
        let metadata = live_metadata(&live).await?;
        let mut active = live.active.lock().await;
        let session_id = active
            .id
            .clone()
            .ok_or_else(|| anyhow::anyhow!("ACP session is not prepared"))?;
        let option = active
            .config_options
            .iter()
            .find(|option| option.id.to_string() == config_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("unknown ACP config option `{config_id}`"))?;
        validate_config_value(&option, &value)?;

        let spawn_arg = option
            .meta
            .as_ref()
            .and_then(|meta| meta.get("aqbotSpawnArg"))
            .and_then(|value| value.as_str());
        if let Some(spawn_arg) = spawn_arg {
            let selected = value
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("spawn configuration value must be a string"))?;
            let updated_agent =
                agent_with_spawn_argument(&live.configured_agent, spawn_arg, selected)?;
            let cwd = live.cwd.clone();
            let auto_approve = live.auto_approve.load(Ordering::Acquire);
            let before_snapshot = snapshot_from_state(&active, &metadata);
            let persisted_mode = persisted_mode_id(&before_snapshot);
            let selections = restorable_config_selections(&active.config_options, config_id);
            drop(active);
            drop(operation);
            drop(admission);
            // This path intentionally replaces the current process. Release
            // the old generation's activity marker before ensure_live performs
            // that replacement; all in-process setter paths keep it held.
            drop(busy_guard);
            let (event_tx, _event_rx) = mpsc::unbounded_channel();
            let replacement_limits = *live
                .runtime_limits
                .lock()
                .map_err(|_| anyhow::anyhow!("ACP runtime limits lock is poisoned"))?;
            self.ensure_live(
                session_key,
                &updated_agent,
                cwd,
                auto_approve,
                replacement_limits,
                &event_tx,
            )
            .await?;
            let mut replacement = self
                .prepare(
                    session_key,
                    &updated_agent,
                    live.cwd.clone(),
                    Some(session_id.to_string()),
                    auto_approve,
                    replacement_limits,
                    event_tx,
                )
                .await?;
            if let Some((_, discovered)) = self.wait_for_capability_discovery(session_key).await? {
                replacement = discovered;
            }
            for (restore_id, restore_value) in selections {
                let Some(candidate) = replacement
                    .config_options
                    .iter()
                    .find(|candidate| candidate.id.to_string() == restore_id)
                else {
                    tracing::warn!(
                        config_id = %restore_id,
                        "replacement ACP session no longer advertises a previous configuration option"
                    );
                    continue;
                };
                let already_selected =
                    current_config_value(candidate).as_ref() == Some(&restore_value);
                if already_selected {
                    continue;
                }
                replacement =
                    Box::pin(self.set_config_option(session_key, &restore_id, restore_value))
                        .await?;
            }
            if let Some(persisted_mode) = persisted_mode {
                if let Some(restored) =
                    Box::pin(self.restore_persisted_mode(session_key, &persisted_mode)).await?
                {
                    replacement = restored;
                }
            }
            return Ok(replacement);
        }

        let set_method = option
            .meta
            .as_ref()
            .and_then(|meta| meta.get("aqbotSetMethod"))
            .and_then(serde_json::Value::as_str);

        if set_method == Some(GROK_PERMISSION_SET_METHOD)
            && option.id.to_string() == GROK_PERMISSION_CONFIG_ID
            && is_grok_shell(&metadata)
        {
            let mode = value
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Grok permission mode must be a string"))?;
            update_select_value(&mut active.config_options, config_id, mode);
            drop(active);
        } else if set_method == Some("session/set_model") {
            let model_id = value
                .as_str()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow::anyhow!("model value must be a non-empty string"))?;
            drop(active);
            connection
                .send_request(LegacySetModelRequest::new(session_id.clone(), model_id))
                .block_task()
                .await
                .map_err(|e| anyhow::anyhow!("session/set_model failed: {e}"))?;
            let mut active = live.active.lock().await;
            update_select_value(&mut active.config_options, config_id, model_id);
        } else if set_method == Some("session/set_model_reasoning") && is_grok_shell(&metadata) {
            let reasoning_effort = value
                .as_str()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow::anyhow!("reasoning effort must be a non-empty string"))?;
            let model_id = active
                .config_options
                .iter()
                .find(|option| option.category == Some(SessionConfigOptionCategory::Model))
                .and_then(current_select_value)
                .or_else(|| {
                    metadata
                        .meta
                        .as_ref()
                        .and_then(|meta| meta.get("modelState"))
                        .and_then(|state| state.get("currentModelId"))
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                })
                .ok_or_else(|| anyhow::anyhow!("Grok did not advertise a current model"))?;
            drop(active);
            connection
                .send_request(LegacySetModelRequest::with_reasoning(
                    session_id.clone(),
                    &model_id,
                    reasoning_effort,
                ))
                .block_task()
                .await
                .map_err(|e| anyhow::anyhow!("session/set_model reasoning update failed: {e}"))?;
            let mut active = live.active.lock().await;
            update_select_value(&mut active.config_options, config_id, reasoning_effort);
        } else {
            let option_value = if let Some(value) = value.as_bool() {
                SessionConfigOptionValue::boolean(value)
            } else if let Some(value) = value.as_str() {
                SessionConfigOptionValue::value_id(value.to_string())
            } else {
                anyhow::bail!("config option value must be a string or boolean");
            };
            drop(active);
            let response = connection
                .send_request(SetSessionConfigOptionRequest::new(
                    session_id,
                    config_id.to_string(),
                    option_value,
                ))
                .block_task()
                .await
                .map_err(|e| anyhow::anyhow!("session/set_config_option failed: {e}"))?;
            let mut active = live.active.lock().await;
            active.config_options = normalized_config_options_for_session(
                response.config_options,
                &metadata,
                &active.config_options,
            );
            if let Some(mode_id) = value.as_str() {
                sync_session_mode_from_config(&mut active, &option, mode_id);
            }
        }

        live.touch();
        let active = live.active.lock().await;
        Ok(snapshot_from_state(&active, &metadata))
    }

    pub async fn set_mode(
        &self,
        session_key: &str,
        mode_id: &str,
    ) -> anyhow::Result<AcpSessionSnapshot> {
        let live = self.live_session(session_key).await?;
        let _admission = live.admission_lock.lock().await;
        if live.prompt_state.load(Ordering::Acquire) != PROMPT_IDLE {
            anyhow::bail!("cannot change ACP session mode while a prompt is running");
        }
        let _busy_guard = BusyGuard::activate(live.busy.clone());
        if !self
            .sessions
            .lock()
            .await
            .get(session_key)
            .is_some_and(|current| current.permission_scope == live.permission_scope)
        {
            anyhow::bail!("ACP session was replaced before the mode update started");
        }
        let _operation = live.operation_lock.lock().await;
        let connection = live_connection(&live).await?;
        let metadata = live_metadata(&live).await?;
        let active = live.active.lock().await;
        let session_id = active
            .id
            .clone()
            .ok_or_else(|| anyhow::anyhow!("ACP session is not prepared"))?;
        let modes = active
            .modes
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("agent does not advertise session modes"))?;
        if !modes
            .available_modes
            .iter()
            .any(|mode| mode.id.to_string() == mode_id)
        {
            anyhow::bail!("unknown ACP session mode `{mode_id}`");
        }
        drop(active);
        connection
            .send_request(SetSessionModeRequest::new(
                session_id,
                SessionModeId::new(mode_id),
            ))
            .block_task()
            .await
            .map_err(|e| anyhow::anyhow!("session/set_mode failed: {e}"))?;
        let mut active = live.active.lock().await;
        if let Some(modes) = active.modes.as_mut() {
            modes.current_mode_id = SessionModeId::new(mode_id);
        }
        sync_mode_config_values(&mut active.config_options, mode_id);
        live.touch();
        Ok(snapshot_from_state(&active, &metadata))
    }

    /// Run a prompt turn, reusing a live agent process when possible.
    ///
    /// - `session_key`: AQBot thread id (stable live-process key)
    /// - `preferred_session_id`: last known ACP session id from DB
    pub async fn prompt(
        &self,
        session_key: &str,
        agent: &ConfiguredAgent,
        cwd: PathBuf,
        input: AcpPromptInput,
        preferred_session_id: Option<String>,
        auto_approve: bool,
        limits: RuntimeLimits,
        event_tx: mpsc::UnboundedSender<AcpEvent>,
    ) -> anyhow::Result<PromptOutcome> {
        self.schedule_prompt(
            session_key,
            agent,
            cwd,
            input,
            preferred_session_id,
            auto_approve,
            limits,
            event_tx,
        )
        .await?
        .wait()
        .await
    }

    /// Prepare a live process and enqueue a prompt without waiting for the turn
    /// to finish. A successful return is the scheduling acceptance boundary.
    pub async fn schedule_prompt(
        &self,
        session_key: &str,
        agent: &ConfiguredAgent,
        cwd: PathBuf,
        input: AcpPromptInput,
        preferred_session_id: Option<String>,
        auto_approve: bool,
        limits: RuntimeLimits,
        event_tx: mpsc::UnboundedSender<AcpEvent>,
    ) -> anyhow::Result<AcpPromptHandle> {
        self.ensure_live(
            session_key,
            agent,
            cwd.clone(),
            auto_approve,
            limits,
            &event_tx,
        )
        .await?;

        for attempt in 0..2 {
            let live = self.live_session(session_key).await?;
            let admission = live.admission_lock.lock().await;
            let is_current = self
                .sessions
                .lock()
                .await
                .get(session_key)
                .is_some_and(|current| current.permission_scope == live.permission_scope);
            if !is_current {
                drop(admission);
                continue;
            }
            let metadata = live_metadata(&live).await?;
            let prompt = prompt_content_blocks(&input, &metadata.capabilities)?;
            let prompt_dispatch = live.prompt_dispatch_lock.lock().await;
            if live
                .prompt_state
                .compare_exchange(
                    PROMPT_IDLE,
                    PROMPT_QUEUED,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_err()
            {
                drop(prompt_dispatch);
                anyhow::bail!("an ACP prompt is already running for this thread");
            }
            let generation = live
                .prompt_generation
                .fetch_add(1, Ordering::AcqRel)
                .wrapping_add(1);

            let (reply_tx, reply_rx) = oneshot::channel();
            live.touch();
            if live
                .job_tx
                .send(PromptJob {
                    cwd: live.cwd.clone(),
                    prompt,
                    preferred_session_id: preferred_session_id.clone(),
                    event_tx: event_tx.clone(),
                    generation,
                    reply: reply_tx,
                })
                .is_err()
            {
                live.prompt_state.store(PROMPT_IDLE, Ordering::Release);
                drop(prompt_dispatch);
                drop(admission);
                remove_session_if_current(
                    self.sessions.as_ref(),
                    session_key,
                    &live.permission_scope,
                )
                .await;
                self.cancel_permissions(&live.permission_scope).await;
                if attempt == 0 {
                    self.ensure_live(
                        session_key,
                        agent,
                        cwd.clone(),
                        auto_approve,
                        limits,
                        &event_tx,
                    )
                    .await?;
                    continue;
                }
                anyhow::bail!("agent session worker is closed");
            }
            drop(prompt_dispatch);
            drop(admission);
            return Ok(AcpPromptHandle {
                session_key: session_key.to_string(),
                permission_scope: live.permission_scope.clone(),
                permissions: self.permissions.clone(),
                sessions: self.sessions.clone(),
                reply_rx,
            });
        }
        anyhow::bail!("ACP session process changed while scheduling the prompt")
    }

    async fn live_session(&self, session_key: &str) -> anyhow::Result<LiveSession> {
        self.sessions
            .lock()
            .await
            .get(session_key)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("ACP session process is not running"))
    }

    async fn ensure_live(
        &self,
        session_key: &str,
        agent: &ConfiguredAgent,
        cwd: PathBuf,
        auto_approve: bool,
        limits: RuntimeLimits,
        event_tx: &mpsc::UnboundedSender<AcpEvent>,
    ) -> anyhow::Result<()> {
        let session_lock = {
            let mut locks = self.session_locks.lock().await;
            locks
                .entry(session_key.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let _session_guard = session_lock.lock().await;
        let pool_guard = self.pool_lock.lock().await;
        let fingerprint = LaunchFingerprint::new(agent, auto_approve);
        let mut map = self.sessions.lock().await;
        let expired = prune_expired_sessions(&mut map, limits.idle_timeout);
        let needs_new = match map.get(session_key) {
            None => true,
            Some(session) => {
                session.fingerprint != fingerprint
                    || session.cwd != cwd
                    || session.job_tx.is_closed()
                    || !session.process_is_healthy()
            }
        };
        if !needs_new {
            let live = map.get(session_key).expect("checked above").clone();
            live.auto_approve.store(auto_approve, Ordering::Release);
            *live
                .runtime_limits
                .lock()
                .map_err(|_| anyhow::anyhow!("ACP runtime limits lock is poisoned"))? = limits;
            live.touch();
            let readiness_guard = BusyGuard::activate(live.busy.clone());
            drop(map);
            drop(pool_guard);
            for expired in expired {
                unregister_live_route(&expired).await;
                self.cancel_permissions(&expired.permission_scope).await;
            }
            let result = wait_until_ready(live.ready.clone()).await;
            drop(readiness_guard);
            return result;
        }
        if map.get(session_key).is_some_and(LiveSession::is_active) {
            anyhow::bail!("cannot replace an active ACP session process");
        }
        let previous = map.get(session_key).cloned();
        let mut warm = self.warm_sessions.lock().await;
        if warm
            .get(&fingerprint)
            .is_some_and(|anchor| !anchor.process_is_healthy())
        {
            warm.remove(&fingerprint);
        }
        let retiring = self
            .retiring_processes
            .lock()
            .expect("ACP retiring processes lock is poisoned")
            .clone();
        let existing_anchor = warm
            .get(&fingerprint)
            .filter(|anchor| !retiring.contains(&anchor.process_scope))
            .cloned()
            .or_else(|| {
                map.values()
                    .find(|live| {
                        live.fingerprint == fingerprint
                            && live.process_is_healthy()
                            && !retiring.contains(&live.process_scope)
                    })
                    .cloned()
            });
        let created_anchor = existing_anchor.is_none();
        let mut evicted_anchor = None;
        let anchor = if let Some(anchor) = existing_anchor {
            let _ = event_tx.send(AcpEvent::Status {
                message: ACP_STATUS_USING_SHARED_AGENT.into(),
            });
            anchor
        } else {
            let reservations = self
                .process_reservations
                .lock()
                .expect("ACP process reservations lock is poisoned")
                .clone();
            evicted_anchor = evict_process_anchor_for_capacity(
                &map,
                &mut warm,
                limits.max_processes,
                Some(session_key),
                &reservations,
            )?;
            if let Some((_, evicted)) = evicted_anchor.as_ref() {
                self.retiring_processes
                    .lock()
                    .expect("ACP retiring processes lock is poisoned")
                    .insert(evicted.process_scope.clone());
            }
            let _ = event_tx.send(AcpEvent::Status {
                message: ACP_STATUS_LAUNCHING_AGENT.into(),
            });
            let anchor =
                match spawn_process_anchor(agent, auto_approve, limits, self.permissions.clone()) {
                    Ok(anchor) => anchor,
                    Err(error) => {
                        if let Some((fingerprint, live)) = evicted_anchor.take() {
                            self.retiring_processes
                                .lock()
                                .expect("ACP retiring processes lock is poisoned")
                                .remove(&live.process_scope);
                            warm.insert(fingerprint, live);
                        }
                        return Err(error);
                    }
                };
            warm.insert(fingerprint.clone(), anchor.clone());
            anchor
        };
        let live = spawn_logical_session(
            &anchor,
            agent,
            cwd,
            auto_approve,
            limits,
            self.permissions.clone(),
        );
        let ready = live.ready.clone();
        let readiness_guard = BusyGuard::activate(live.busy.clone());
        let process_scope = live.process_scope.clone();

        if let Some(previous) = previous {
            self.process_reservations
                .lock()
                .expect("ACP process reservations lock is poisoned")
                .insert(process_scope.clone());
            drop(warm);
            drop(map);
            drop(pool_guard);
            let replacement_admission = previous.admission_lock.lock().await;
            let current_matches = self
                .sessions
                .lock()
                .await
                .get(session_key)
                .is_some_and(|current| current.permission_scope == previous.permission_scope);
            if !current_matches || previous.is_active() {
                drop(replacement_admission);
                drop(readiness_guard);
                let removed = self
                    .rollback_process_candidate(
                        &process_scope,
                        created_anchor,
                        evicted_anchor.take(),
                    )
                    .await;
                for expired in expired {
                    unregister_live_route(&expired).await;
                    self.cancel_permissions(&expired.permission_scope).await;
                }
                for removed in removed {
                    unregister_live_route(&removed).await;
                    self.cancel_permissions(&removed.permission_scope).await;
                }
                anyhow::bail!("ACP session changed while its replacement was starting");
            }
            if let Err(error) = wait_until_ready(ready).await {
                drop(replacement_admission);
                drop(readiness_guard);
                let removed = self
                    .rollback_process_candidate(&process_scope, true, evicted_anchor.take())
                    .await;
                for expired in expired {
                    unregister_live_route(&expired).await;
                    self.cancel_permissions(&expired.permission_scope).await;
                }
                for removed in removed {
                    unregister_live_route(&removed).await;
                    self.cancel_permissions(&removed.permission_scope).await;
                }
                return Err(error);
            }
            let commit_pool = self.pool_lock.lock().await;
            let mut map = self.sessions.lock().await;
            let current_matches = map
                .get(session_key)
                .is_some_and(|current| current.permission_scope == previous.permission_scope);
            if !current_matches || previous.is_active() {
                drop(map);
                drop(commit_pool);
                drop(replacement_admission);
                drop(readiness_guard);
                let removed = self
                    .rollback_process_candidate(
                        &process_scope,
                        created_anchor,
                        evicted_anchor.take(),
                    )
                    .await;
                for expired in expired {
                    unregister_live_route(&expired).await;
                    self.cancel_permissions(&expired.permission_scope).await;
                }
                for removed in removed {
                    unregister_live_route(&removed).await;
                    self.cancel_permissions(&removed.permission_scope).await;
                }
                anyhow::bail!("ACP session changed while its replacement was starting");
            }
            let replaced = map.remove(session_key).expect("replacement checked above");
            map.insert(session_key.to_string(), live);
            self.process_reservations
                .lock()
                .expect("ACP process reservations lock is poisoned")
                .remove(&process_scope);
            drop(map);
            drop(commit_pool);
            drop(replacement_admission);
            drop(readiness_guard);
            for expired in expired {
                unregister_live_route(&expired).await;
                self.cancel_permissions(&expired.permission_scope).await;
            }
            unregister_live_route(&replaced).await;
            self.cancel_permissions(&replaced.permission_scope).await;
            self.finalize_evicted_anchor(evicted_anchor.take());
            let _ = event_tx.send(AcpEvent::Status {
                message: ACP_STATUS_AGENT_READY.into(),
            });
            return Ok(());
        }

        map.insert(session_key.to_string(), live);
        drop(warm);
        drop(map);
        drop(pool_guard);
        for expired in expired {
            unregister_live_route(&expired).await;
            self.cancel_permissions(&expired.permission_scope).await;
        }
        if let Err(error) = wait_until_ready(ready).await {
            let removed = self
                .rollback_process_candidate(&process_scope, true, evicted_anchor.take())
                .await;
            for removed in removed {
                unregister_live_route(&removed).await;
                self.cancel_permissions(&removed.permission_scope).await;
            }
            drop(readiness_guard);
            return Err(error);
        }
        drop(readiness_guard);
        self.finalize_evicted_anchor(evicted_anchor.take());
        let _ = event_tx.send(AcpEvent::Status {
            message: ACP_STATUS_AGENT_READY.into(),
        });
        Ok(())
    }

    async fn cancel_permissions(&self, scope: &str) {
        cancel_permission_scope(&self.permissions, scope).await;
    }

    async fn shutdown_process_scope(&self, live: &LiveSession) {
        live.process_shutdown.store(true, Ordering::Release);
        live.process_abort.abort();
        *live.connection.lock().await = None;

        let _pool = self.pool_lock.lock().await;
        let mut sessions = self.sessions.lock().await;
        let mut warm = self.warm_sessions.lock().await;
        self.process_reservations
            .lock()
            .expect("ACP process reservations lock is poisoned")
            .remove(&live.process_scope);
        let removed = remove_process_scope(&mut sessions, &mut warm, &live.process_scope);
        drop(warm);
        drop(sessions);
        drop(_pool);

        for removed in removed {
            unregister_live_route(&removed).await;
            self.cancel_permissions(&removed.permission_scope).await;
        }
    }

    fn finalize_evicted_anchor(&self, evicted_anchor: Option<(LaunchFingerprint, LiveSession)>) {
        let Some((_, live)) = evicted_anchor else {
            return;
        };
        self.retiring_processes
            .lock()
            .expect("ACP retiring processes lock is poisoned")
            .remove(&live.process_scope);
        live.process_shutdown.store(true, Ordering::Release);
        live.process_abort.abort();
    }

    async fn rollback_process_candidate(
        &self,
        process_scope: &str,
        remove_candidate: bool,
        evicted_anchor: Option<(LaunchFingerprint, LiveSession)>,
    ) -> Vec<LiveSession> {
        let _pool = self.pool_lock.lock().await;
        let mut sessions = self.sessions.lock().await;
        let mut warm = self.warm_sessions.lock().await;
        self.process_reservations
            .lock()
            .expect("ACP process reservations lock is poisoned")
            .remove(process_scope);
        let removed = if remove_candidate {
            remove_process_scope(&mut sessions, &mut warm, process_scope)
        } else {
            Vec::new()
        };
        if let Some((fingerprint, live)) = evicted_anchor {
            self.retiring_processes
                .lock()
                .expect("ACP retiring processes lock is poisoned")
                .remove(&live.process_scope);
            if live.process_is_healthy() {
                warm.entry(fingerprint).or_insert(live);
            }
        }
        removed
    }
}

async fn remove_session_if_current(
    sessions: &Mutex<HashMap<String, LiveSession>>,
    session_key: &str,
    permission_scope: &str,
) {
    let mut sessions = sessions.lock().await;
    let removed = if sessions
        .get(session_key)
        .is_some_and(|live| live.permission_scope == permission_scope)
    {
        sessions.remove(session_key)
    } else {
        None
    };
    drop(sessions);
    if let Some(live) = removed {
        unregister_live_route(&live).await;
    }
}

async fn wait_for_prompt_completion(live: &LiveSession, generation: u64, grace: Duration) -> bool {
    if live.completed_generation.load(Ordering::Acquire) >= generation {
        return true;
    }
    let mut completion = live.completion_tx.subscribe();
    tokio::time::timeout(grace, async {
        loop {
            if live.completed_generation.load(Ordering::Acquire) >= generation
                || *completion.borrow() >= generation
            {
                return true;
            }
            if completion.changed().await.is_err() {
                return false;
            }
        }
    })
    .await
    .unwrap_or(false)
}

impl LiveSession {
    fn route(&self) -> SessionRoute {
        SessionRoute {
            active: self.active.clone(),
            event_slot: self.event_slot.clone(),
            auto_approve: self.auto_approve.clone(),
            prompt_state: self.prompt_state.clone(),
            prompt_dispatch_lock: self.prompt_dispatch_lock.clone(),
            permission_scope: self.permission_scope.clone(),
        }
    }

    fn process_is_healthy(&self) -> bool {
        !self.process_shutdown.load(Ordering::Acquire)
            && !self.process_keepalive.is_closed()
            && !matches!(*self.ready.borrow(), ReadyState::Failed(_))
    }

    fn touch(&self) {
        let now = Instant::now();
        if let Ok(mut last_used) = self.last_used.lock() {
            *last_used = now;
        }
        if let Ok(mut last_used) = self.process_last_used.lock() {
            *last_used = now;
        }
    }

    fn idle_for(&self) -> Duration {
        self.last_used
            .lock()
            .map(|last_used| last_used.elapsed())
            .unwrap_or_default()
    }

    fn is_active(&self) -> bool {
        matches!(*self.ready.borrow(), ReadyState::Starting)
            || self.busy.load(Ordering::Acquire) > 0
            || self.prompt_state.load(Ordering::Acquire) != PROMPT_IDLE
    }

    fn process_idle_for(&self) -> Duration {
        self.process_last_used
            .lock()
            .map(|last_used| last_used.elapsed())
            .unwrap_or_default()
    }
}

fn prune_expired_sessions(
    sessions: &mut HashMap<String, LiveSession>,
    idle_timeout: Duration,
) -> Vec<LiveSession> {
    if idle_timeout.is_zero() {
        return Vec::new();
    }
    let expired_keys = sessions
        .iter()
        .filter(|(_, live)| !live.is_active() && live.idle_for() >= idle_timeout)
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    expired_keys
        .into_iter()
        .filter_map(|key| sessions.remove(&key))
        .collect()
}

fn evict_process_anchor_for_capacity(
    sessions: &HashMap<String, LiveSession>,
    warm: &mut HashMap<LaunchFingerprint, LiveSession>,
    max_processes: usize,
    excluded_session_key: Option<&str>,
    reserved_processes: &HashSet<String>,
) -> anyhow::Result<Option<(LaunchFingerprint, LiveSession)>> {
    if max_processes == 0 || warm.len() < max_processes {
        return Ok(None);
    }
    if warm.len() > max_processes {
        anyhow::bail!(
            "maximum concurrent ACP processes reached ({max_processes}); {} processes are still retained",
            warm.len()
        );
    }
    let mut in_use = sessions
        .iter()
        .filter(|(session_key, _)| Some(session_key.as_str()) != excluded_session_key)
        .map(|(_, live)| live.process_scope.clone())
        .collect::<HashSet<_>>();
    in_use.extend(reserved_processes.iter().cloned());
    let candidate = warm
        .iter()
        .filter(|(_, live)| !live.is_active() && !in_use.contains(&live.process_scope))
        .max_by_key(|(_, live)| live.process_idle_for())
        .map(|(fingerprint, _)| fingerprint.clone());
    if let Some(candidate) = candidate {
        let live = warm
            .remove(&candidate)
            .expect("capacity candidate came from warm process map");
        return Ok(Some((candidate, live)));
    }
    anyhow::bail!("maximum concurrent ACP processes reached ({max_processes})")
}

fn remove_process_scope(
    sessions: &mut HashMap<String, LiveSession>,
    warm: &mut HashMap<LaunchFingerprint, LiveSession>,
    process_scope: &str,
) -> Vec<LiveSession> {
    let keys = sessions
        .iter()
        .filter(|(_, live)| live.process_scope == process_scope)
        .map(|(session_key, _)| session_key.clone())
        .collect::<Vec<_>>();
    let removed = keys
        .into_iter()
        .filter_map(|session_key| sessions.remove(&session_key))
        .collect::<Vec<_>>();
    warm.retain(|_, live| live.process_scope != process_scope);
    removed
}

async fn unregister_live_route(live: &LiveSession) {
    let session_id = live.active.lock().await.id.clone().map(|id| id.to_string());
    let mut routes = live.routes.lock().await;
    if let Some(session_id) = session_id {
        if routes
            .by_session_id
            .get(&session_id)
            .is_some_and(|route| route.permission_scope == live.permission_scope)
        {
            routes.by_session_id.remove(&session_id);
        }
    }
    if routes
        .opening
        .as_ref()
        .is_some_and(|route| route.permission_scope == live.permission_scope)
    {
        routes.opening = None;
    }
}

async fn wait_until_ready(mut ready: watch::Receiver<ReadyState>) -> anyhow::Result<()> {
    tokio::time::timeout(Duration::from_secs(120), async move {
        loop {
            let state = ready.borrow().clone();
            match state {
                ReadyState::Starting => {
                    ready
                        .changed()
                        .await
                        .map_err(|_| anyhow::anyhow!("agent process exited during startup"))?;
                }
                ReadyState::Ready => return Ok(()),
                ReadyState::Failed(message) => anyhow::bail!(message),
            }
        }
    })
    .await
    .map_err(|_| anyhow::anyhow!("agent initialize timed out"))?
}

fn nested_agent_error_data(raw: &str) -> Option<String> {
    fn strip_dependency_wrappers(value: serde_json::Value) -> (serde_json::Value, bool) {
        match value {
            serde_json::Value::Object(mut object)
                if object
                    .get("spawned_at")
                    .is_some_and(|value| value.is_string())
                    && object.contains_key("data") =>
            {
                let data = object.remove("data").expect("checked data field");
                let (data, _) = strip_dependency_wrappers(data);
                (data, true)
            }
            serde_json::Value::Object(object) => {
                let mut found_wrapper = false;
                let mut sanitized = serde_json::Map::new();
                for (key, value) in object {
                    let (value, found) = strip_dependency_wrappers(value);
                    found_wrapper |= found;
                    sanitized.insert(key, value);
                }
                (serde_json::Value::Object(sanitized), found_wrapper)
            }
            serde_json::Value::Array(values) => {
                let mut found_wrapper = false;
                let values = values
                    .into_iter()
                    .map(|value| {
                        let (value, found) = strip_dependency_wrappers(value);
                        found_wrapper |= found;
                        value
                    })
                    .collect();
                (serde_json::Value::Array(values), found_wrapper)
            }
            value => (value, false),
        }
    }

    let value = serde_json::from_str::<serde_json::Value>(&raw[raw.find('{')?..]).ok()?;
    let (value, found_wrapper) = strip_dependency_wrappers(value);
    if !found_wrapper {
        return None;
    }
    match value {
        serde_json::Value::String(data) => Some(data),
        value => serde_json::to_string(&value).ok(),
    }
}

/// Pull a human-readable reason out of agent-client-protocol / npm spawn errors.
fn summarize_agent_spawn_error(raw: &str, command: &str) -> String {
    let nested = nested_agent_error_data(raw);
    let raw = nested.as_deref().unwrap_or(raw);
    // Prefer the nested "data": "Process exited … npm error …" payload when present.
    if let Some(idx) = raw.find("npm error") {
        let slice = &raw[idx..];
        let cleaned = slice
            .replace("\\n", "\n")
            .replace("\\\"", "\"")
            .lines()
            .filter(|l| !l.trim().is_empty())
            .take(4)
            .collect::<Vec<_>>()
            .join(" ");
        if !cleaned.is_empty() {
            return cleaned.chars().take(400).collect();
        }
    }
    if let Some(idx) = raw.find("Process exited") {
        return raw[idx..].chars().take(400).collect();
    }
    let trimmed = raw.trim();
    let lowercase = trimmed.to_ascii_lowercase();
    if lowercase.contains("os error 2")
        || lowercase.contains("no such file or directory")
        || lowercase.contains("cannot find the file specified")
    {
        return format!("failed to start `{command}`: {trimmed}");
    }
    if trimmed.chars().count() > 400 {
        format!("{}…", trimmed.chars().take(400).collect::<String>())
    } else if trimmed.is_empty() {
        "unknown error".into()
    } else {
        trimmed.to_string()
    }
}

fn configured_agent_for_process_with_path(
    agent: &ConfiguredAgent,
    shell_path: &str,
) -> ConfiguredAgent {
    let mut configured = agent.clone();
    crate::shell_path::inject_shell_path(&mut configured.env, shell_path);
    configured
}

fn configured_agent_for_process(agent: &ConfiguredAgent) -> ConfiguredAgent {
    configured_agent_for_process_with_path(agent, crate::shell_path::get_shell_path())
}

fn build_acp_agent(agent: &ConfiguredAgent) -> AcpAgent {
    AcpAgent::new(
        AcpAgentConfig::new(&agent.command)
            .args(agent.args.clone())
            .envs(agent.env.clone()),
    )
}

fn spawn_process_anchor(
    agent: &ConfiguredAgent,
    auto_approve: bool,
    limits: RuntimeLimits,
    permissions: PermissionMap,
) -> anyhow::Result<LiveSession> {
    let process_agent = configured_agent_for_process(agent);
    let acp_agent = build_acp_agent(&process_agent);

    let (keepalive_tx, mut keepalive_rx) = mpsc::unbounded_channel::<PromptJob>();
    let (ready_tx, ready_rx) = watch::channel(ReadyState::Starting);
    let (discovery_tx, discovery_rx) = watch::channel(false);
    let agent_id = agent.id.clone();
    let agent_name = agent.name.clone();
    let agent_command = agent.command.clone();
    let agent_for_discovery = process_agent;
    let event_slot: EventTxSlot = Arc::new(Mutex::new(None));
    let connection: ConnectionSlot = Arc::new(Mutex::new(None));
    let metadata: Arc<Mutex<Option<AgentMetadata>>> = Arc::new(Mutex::new(None));
    let routes: RouteMap = Arc::new(Mutex::new(SessionRoutes::default()));
    let session_open_lock = Arc::new(Mutex::new(()));
    let process_operation_lock = Arc::new(Mutex::new(()));
    let process_last_used = Arc::new(StdMutex::new(Instant::now()));
    let active = Arc::new(Mutex::new(ActiveSession::default()));
    let admission_lock = Arc::new(Mutex::new(()));
    let operation_lock = Arc::new(Mutex::new(()));
    let auto_approve = Arc::new(AtomicBool::new(auto_approve));
    let busy = Arc::new(AtomicUsize::new(0));
    let prompt_state = Arc::new(AtomicU8::new(PROMPT_IDLE));
    let prompt_dispatch_lock = Arc::new(Mutex::new(()));
    let prompt_generation = Arc::new(AtomicU64::new(0));
    let completed_generation = Arc::new(AtomicU64::new(0));
    let (completion_tx, _completion_rx) = watch::channel(0);
    let (cancel_tx, _cancel_rx) = watch::channel(0);
    let process_shutdown = Arc::new(AtomicBool::new(false));
    let permission_scope = uuid::Uuid::new_v4().to_string();
    let process_scope = uuid::Uuid::new_v4().to_string();
    let fingerprint = LaunchFingerprint::new(agent, auto_approve.load(Ordering::Acquire));

    // Dispatch callbacks only enqueue work. This prevents an early update sent
    // before session/new's response from deadlocking the JSON-RPC reader.
    let (notification_tx, mut notification_rx) = mpsc::unbounded_channel::<NotificationWork>();
    let notification_barrier_tx = notification_tx.clone();
    let notification_routes = routes.clone();
    let notification_metadata = metadata.clone();
    tokio::spawn(async move {
        while let Some(work) = notification_rx.recv().await {
            match work {
                NotificationWork::Session(notification) => {
                    route_session_notification(
                        notification,
                        &notification_routes,
                        &notification_metadata,
                    )
                    .await;
                }
                NotificationWork::Extension(notification) => {
                    route_extension_notification(notification, &notification_routes).await;
                }
                NotificationWork::Barrier(done) => {
                    let _ = done.send(());
                }
            }
        }
    });

    let connection_worker = connection.clone();
    let metadata_worker = metadata.clone();
    let routes_worker = routes.clone();
    let process_shutdown_worker = process_shutdown.clone();

    let connection_task = tokio::spawn(async move {
        let permissions_perm = permissions.clone();
        let permissions_plan = permissions.clone();
        let permissions_question = permissions.clone();
        let ready_tx_fallback = ready_tx.clone();
        let connection_slot = connection_worker;
        let metadata_slot = metadata_worker;
        let routes = routes_worker;
        let agent_for_discovery = agent_for_discovery;
        let discovery_tx = discovery_tx;

        let connect_result = agent_client_protocol::Client
            .builder()
            .name("aqbot")
            .on_close(async |_connection| {
                Err(agent_client_protocol::util::internal_error(
                    "agent transport closed",
                ))
            })
            .on_receive_notification(
                {
                    let notification_tx = notification_tx;
                    move |notification: AgentNotification, _cx| {
                        let queued = match notification {
                            AgentNotification::SessionNotification(notification) => {
                                notification_tx.send(NotificationWork::Session(notification))
                            }
                            AgentNotification::ExtNotification(notification) => {
                                notification_tx.send(NotificationWork::Extension(notification))
                            }
                            _ => Ok(()),
                        };
                        async move {
                            queued.map_err(|_| {
                                agent_client_protocol::util::internal_error(
                                    "ACP notification state worker exited",
                                )
                            })
                        }
                    }
                },
                agent_client_protocol::on_receive_notification!(),
            )
            .on_receive_request(
                {
                    let permissions = permissions_perm;
                    let routes = routes.clone();
                    move |request: RequestPermissionRequest,
                          responder: Responder<RequestPermissionResponse>,
                          _connection: ConnectionTo<Agent>| {
                        let permissions = permissions.clone();
                        let routes = routes.clone();
                        let connection = _connection.clone();
                        async move {
                            // Permission waits can last minutes. Keep them off the ACP
                            // connection event loop so stream/cancel traffic stays responsive.
                            connection.spawn(async move {
                                let route = resolve_session_route(&routes, &request.session_id).await;
                                if let Some(route) = route {
                                    if route.prompt_state.load(Ordering::Acquire)
                                        == PROMPT_CANCEL_REQUESTED
                                    {
                                        return responder.respond(RequestPermissionResponse::new(
                                            RequestPermissionOutcome::Cancelled,
                                        ));
                                    }
                                    let event_tx = route.event_slot.lock().await.clone();
                                    handle_permission_request(
                                        request,
                                        responder,
                                        route.auto_approve.load(Ordering::Acquire),
                                        permissions,
                                        route.permission_scope,
                                        event_tx,
                                        route.prompt_state,
                                        route.prompt_dispatch_lock,
                                    )
                                    .await
                                } else {
                                    responder.respond(RequestPermissionResponse::new(
                                        RequestPermissionOutcome::Cancelled,
                                    ))
                                }
                            })?;
                            Ok(())
                        }
                    }
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                {
                    let permissions = permissions_plan;
                    let routes = routes.clone();
                    move |request: GrokExitPlanModeRequest,
                          responder: Responder<GrokExitPlanModeResponse>,
                          connection: ConnectionTo<Agent>| {
                        let permissions = permissions.clone();
                        let routes = routes.clone();
                        async move {
                            connection.spawn(async move {
                                let route = resolve_session_route(&routes, &request.session_id).await;
                                if let Some(route) = route {
                                    if route.prompt_state.load(Ordering::Acquire)
                                        == PROMPT_CANCEL_REQUESTED
                                    {
                                        return responder.respond(GrokExitPlanModeResponse::new(
                                            "cancelled",
                                        ));
                                    }
                                    let event_tx = route.event_slot.lock().await.clone();
                                    handle_grok_exit_plan_mode(
                                        request,
                                        responder,
                                        permissions,
                                        route.permission_scope,
                                        event_tx,
                                        route.prompt_state,
                                        route.prompt_dispatch_lock,
                                    )
                                    .await
                                } else {
                                    responder.respond(GrokExitPlanModeResponse::new("cancelled"))
                                }
                            })?;
                            Ok(())
                        }
                    }
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                {
                    let permissions = permissions_question;
                    let routes = routes.clone();
                    move |request: GrokAskUserRequest,
                          responder: Responder<GrokAskUserResponse>,
                          connection: ConnectionTo<Agent>| {
                        let permissions = permissions.clone();
                        let routes = routes.clone();
                        async move {
                            connection.spawn(async move {
                                let route = resolve_session_route(&routes, &request.session_id).await;
                                if let Some(route) = route {
                                    if route.prompt_state.load(Ordering::Acquire)
                                        == PROMPT_CANCEL_REQUESTED
                                    {
                                        return responder.respond(GrokAskUserResponse::cancelled());
                                    }
                                    let event_tx = route.event_slot.lock().await.clone();
                                    handle_grok_ask_user(
                                        request,
                                        responder,
                                        permissions,
                                        route.permission_scope,
                                        event_tx,
                                        route.prompt_state,
                                        route.prompt_dispatch_lock,
                                    )
                                    .await
                                } else {
                                    responder.respond(GrokAskUserResponse::cancelled())
                                }
                            })?;
                            Ok(())
                        }
                    }
                },
                agent_client_protocol::on_receive_request!(),
            )
            .connect_with(acp_agent, {
                let ready_tx = ready_tx.clone();
                let connection_slot = connection_slot.clone();
                let metadata_slot = metadata_slot.clone();
                let routes = routes.clone();
                let agent_for_discovery = agent_for_discovery.clone();
                move |connection: ConnectionTo<Agent>| {
                    let ready_tx = ready_tx.clone();
                    let connection_slot = connection_slot.clone();
                    let metadata_slot = metadata_slot.clone();
                    let routes = routes.clone();
                    let agent_for_discovery = agent_for_discovery.clone();
                    async move {
                        // Initialize once per process.
                        let initialize = InitializeRequest::new(ProtocolVersion::V1)
                            .client_capabilities(aqbot_client_capabilities());
                        match connection.send_request(initialize).block_task().await {
                            Ok(response) => {
                                *metadata_slot.lock().await = Some(AgentMetadata {
                                    capabilities: response.agent_capabilities,
                                    meta: response.meta,
                                    launch_config_options: Vec::new(),
                                });
                                *connection_slot.lock().await = Some(connection.clone());
                                let _ = ready_tx.send(ReadyState::Ready);

                                // Optional CLI catalog discovery must never delay the ACP
                                // handshake. Standard initialize/session data is authoritative;
                                // this background probe only fills gaps such as Copilot's
                                // startup-level model and reasoning selectors.
                                let metadata_for_discovery = metadata_slot.clone();
                                let routes_for_discovery = routes.clone();
                                tokio::spawn(async move {
                                    match discover_launch_config_options(&agent_for_discovery).await {
                                        Ok(options) if !options.is_empty() => {
                                            let metadata = {
                                                let mut slot = metadata_for_discovery.lock().await;
                                                let Some(metadata) = slot.as_mut() else {
                                                    return;
                                                };
                                                metadata.launch_config_options = options;
                                                metadata.clone()
                                            };
                                            refresh_routed_config_options(
                                                &routes_for_discovery,
                                                &metadata,
                                            )
                                            .await;
                                        }
                                        Ok(_) => {}
                                        Err(error) => tracing::warn!(
                                            %error,
                                            agent = %agent_for_discovery.id,
                                            "ACP connected, but optional launch capability discovery failed"
                                        ),
                                    }
                                    let _ = discovery_tx.send(true);
                                });
                            }
                            Err(error) => {
                                let msg = format!("initialize failed: {error}");
                                let _ = ready_tx.send(ReadyState::Failed(msg.clone()));
                                return Err(agent_client_protocol::util::internal_error(msg));
                            }
                        }

                        while keepalive_rx.recv().await.is_some() {}

                        Ok(())
                    }
                }
            })
            .await;

        process_shutdown_worker.store(true, Ordering::Release);
        if let Err(e) = connect_result {
            let detail = summarize_agent_spawn_error(&e.to_string(), &agent_command);
            tracing::warn!(
                error = %e,
                agent = %agent_name,
                "acp live session exited"
            );
            let _ = ready_tx_fallback.send(ReadyState::Failed(format!(
                "agent process exited: {detail}"
            )));
        }
    });
    let process_abort = Arc::new(connection_task.abort_handle());

    Ok(LiveSession {
        job_tx: keepalive_tx.clone(),
        process_keepalive: keepalive_tx,
        fingerprint,
        process_scope,
        agent_id,
        configured_agent: agent.clone(),
        cwd: PathBuf::new(),
        ready: ready_rx,
        discovery_ready: discovery_rx,
        connection,
        metadata,
        routes,
        notification_barrier_tx,
        session_open_lock,
        process_operation_lock,
        event_slot,
        active,
        admission_lock,
        operation_lock,
        auto_approve,
        busy,
        prompt_state,
        prompt_dispatch_lock,
        prompt_generation,
        completed_generation,
        completion_tx,
        cancel_tx,
        process_shutdown,
        process_abort,
        runtime_limits: Arc::new(StdMutex::new(limits)),
        last_used: Arc::new(StdMutex::new(Instant::now())),
        process_last_used,
        permission_scope,
    })
}

fn spawn_logical_session(
    anchor: &LiveSession,
    agent: &ConfiguredAgent,
    cwd: PathBuf,
    auto_approve: bool,
    limits: RuntimeLimits,
    _permissions: PermissionMap,
) -> LiveSession {
    let (job_tx, mut job_rx) = mpsc::unbounded_channel::<PromptJob>();
    let event_slot: EventTxSlot = Arc::new(Mutex::new(None));
    let active = Arc::new(Mutex::new(ActiveSession::default()));
    let admission_lock = Arc::new(Mutex::new(()));
    let operation_lock = Arc::new(Mutex::new(()));
    let auto_approve = Arc::new(AtomicBool::new(auto_approve));
    let busy = Arc::new(AtomicUsize::new(0));
    let prompt_state = Arc::new(AtomicU8::new(PROMPT_IDLE));
    let prompt_dispatch_lock = Arc::new(Mutex::new(()));
    let prompt_generation = Arc::new(AtomicU64::new(0));
    let completed_generation = Arc::new(AtomicU64::new(0));
    let (completion_tx, _completion_rx) = watch::channel(0);
    let (cancel_tx, _cancel_rx) = watch::channel(0);
    let permission_scope = uuid::Uuid::new_v4().to_string();
    let route = SessionRoute {
        active: active.clone(),
        event_slot: event_slot.clone(),
        auto_approve: auto_approve.clone(),
        prompt_state: prompt_state.clone(),
        prompt_dispatch_lock: prompt_dispatch_lock.clone(),
        permission_scope: permission_scope.clone(),
    };

    let worker_ready = anchor.ready.clone();
    let worker_connection = anchor.connection.clone();
    let worker_metadata = anchor.metadata.clone();
    let worker_routes = anchor.routes.clone();
    let worker_open_lock = anchor.session_open_lock.clone();
    let worker_process_lock = anchor.process_operation_lock.clone();
    let worker_barrier = anchor.notification_barrier_tx.clone();
    let worker_event_slot = event_slot.clone();
    let worker_active = active.clone();
    let worker_operation_lock = operation_lock.clone();
    let worker_auto_approve = auto_approve.clone();
    let worker_busy = busy.clone();
    let worker_prompt_state = prompt_state.clone();
    let worker_prompt_dispatch_lock = prompt_dispatch_lock.clone();
    let worker_completed_generation = completed_generation.clone();
    let worker_completion_tx = completion_tx.clone();
    let worker_cancel_tx = cancel_tx.clone();
    let worker_route = route.clone();
    tokio::spawn(async move {
        while let Some(job) = job_rx.recv().await {
            let mut cancel_rx = worker_cancel_tx.subscribe();
            let operation = tokio::select! {
                guard = worker_operation_lock.lock() => Ok(Some(guard)),
                cancelled = cancel_rx.wait_for(|cancelled| *cancelled >= job.generation) => {
                    cancelled
                        .map(|_| None)
                        .map_err(|_| anyhow::anyhow!("ACP prompt cancellation channel closed"))
                }
            };
            let _operation = match operation {
                Ok(Some(operation)) => operation,
                Ok(None) => {
                    let result = cancelled_logical_outcome(&worker_active, &worker_metadata).await;
                    finish_prompt_job(
                        job,
                        result,
                        &worker_active,
                        &worker_prompt_state,
                        &worker_completed_generation,
                        &worker_completion_tx,
                    )
                    .await;
                    continue;
                }
                Err(error) => {
                    finish_prompt_job(
                        job,
                        Err(error),
                        &worker_active,
                        &worker_prompt_state,
                        &worker_completed_generation,
                        &worker_completion_tx,
                    )
                    .await;
                    continue;
                }
            };
            let busy_guard = BusyGuard::activate(worker_busy.clone());
            *worker_event_slot.lock().await = Some(job.event_tx.clone());
            let process_operation = tokio::select! {
                guard = worker_process_lock.lock() => Ok(Some(guard)),
                cancelled = cancel_rx.wait_for(|cancelled| *cancelled >= job.generation) => {
                    cancelled
                        .map(|_| None)
                        .map_err(|_| anyhow::anyhow!("ACP prompt cancellation channel closed"))
                }
            };
            let mut result = match process_operation {
                Ok(None) => cancelled_logical_outcome(&worker_active, &worker_metadata).await,
                Err(error) => Err(error),
                Ok(Some(_process_operation)) => {
                    match wait_until_ready(worker_ready.clone()).await {
                        Ok(()) => {
                            let connection = worker_connection
                                .lock()
                                .await
                                .clone()
                                .ok_or_else(|| anyhow::anyhow!("ACP connection is not ready"));
                            match connection {
                                Ok(connection) => {
                                    run_one_prompt(
                                        &connection,
                                        &job.cwd,
                                        &job.prompt,
                                        job.preferred_session_id.as_deref(),
                                        &worker_active,
                                        &worker_metadata,
                                        &worker_auto_approve,
                                        &job.event_tx,
                                        &worker_routes,
                                        &worker_open_lock,
                                        &worker_route,
                                        &worker_prompt_state,
                                        &worker_prompt_dispatch_lock,
                                    )
                                    .await
                                }
                                Err(error) => Err(error),
                            }
                        }
                        Err(error) => Err(error),
                    }
                }
            };

            if let Err(error) = drain_notification_work(&worker_barrier).await {
                result = Err(error);
            }
            *worker_event_slot.lock().await = None;
            drop(busy_guard);
            finish_prompt_job(
                job,
                result,
                &worker_active,
                &worker_prompt_state,
                &worker_completed_generation,
                &worker_completion_tx,
            )
            .await;
        }
    });

    LiveSession {
        job_tx,
        process_keepalive: anchor.process_keepalive.clone(),
        fingerprint: LaunchFingerprint::new(agent, auto_approve.load(Ordering::Acquire)),
        process_scope: anchor.process_scope.clone(),
        agent_id: agent.id.clone(),
        configured_agent: agent.clone(),
        cwd,
        ready: anchor.ready.clone(),
        discovery_ready: anchor.discovery_ready.clone(),
        connection: anchor.connection.clone(),
        metadata: anchor.metadata.clone(),
        routes: anchor.routes.clone(),
        notification_barrier_tx: anchor.notification_barrier_tx.clone(),
        session_open_lock: anchor.session_open_lock.clone(),
        process_operation_lock: anchor.process_operation_lock.clone(),
        event_slot,
        active,
        admission_lock,
        operation_lock,
        auto_approve,
        busy,
        prompt_state,
        prompt_dispatch_lock,
        prompt_generation,
        completed_generation,
        completion_tx,
        cancel_tx,
        process_shutdown: anchor.process_shutdown.clone(),
        process_abort: anchor.process_abort.clone(),
        runtime_limits: Arc::new(StdMutex::new(limits)),
        last_used: Arc::new(StdMutex::new(Instant::now())),
        process_last_used: anchor.process_last_used.clone(),
        permission_scope,
    }
}

async fn finish_prompt_job(
    job: PromptJob,
    result: anyhow::Result<PromptOutcome>,
    active: &Arc<Mutex<ActiveSession>>,
    prompt_state: &Arc<AtomicU8>,
    completed_generation: &Arc<AtomicU64>,
    completion_tx: &watch::Sender<u64>,
) {
    let completion = match &result {
        Ok(outcome) => AcpEvent::Done {
            stop_reason: outcome.stop_reason.clone(),
            session_id: outcome.session_id.clone(),
        },
        Err(error) => AcpEvent::Done {
            stop_reason: format!("error: {error}"),
            session_id: active
                .lock()
                .await
                .id
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default(),
        },
    };
    let _ = job.event_tx.send(completion);
    completed_generation.store(job.generation, Ordering::Release);
    completion_tx.send_replace(job.generation);
    prompt_state.store(PROMPT_IDLE, Ordering::Release);
    let _ = job.reply.send(result);
}

async fn cancelled_logical_outcome(
    active: &Arc<Mutex<ActiveSession>>,
    metadata: &Arc<Mutex<Option<AgentMetadata>>>,
) -> anyhow::Result<PromptOutcome> {
    let metadata = metadata
        .lock()
        .await
        .clone()
        .ok_or_else(|| anyhow::anyhow!("ACP agent metadata is not ready"))?;
    let active = active.lock().await;
    let snapshot = snapshot_from_state(&active, &metadata);
    Ok(PromptOutcome {
        session_id: snapshot.session_id.clone(),
        stop_reason: "cancelled".into(),
        snapshot,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcRequest)]
#[request(method = "session/set_model", response = LegacySetModelResponse)]
#[serde(rename_all = "camelCase")]
struct LegacySetModelRequest {
    session_id: SessionId,
    model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "_meta")]
    meta: Option<agent_client_protocol::schema::v1::Meta>,
}

impl LegacySetModelRequest {
    fn new(session_id: SessionId, model_id: &str) -> Self {
        Self {
            session_id,
            model_id: model_id.to_string(),
            meta: None,
        }
    }

    fn with_reasoning(session_id: SessionId, model_id: &str, reasoning_effort: &str) -> Self {
        let mut meta = agent_client_protocol::schema::v1::Meta::new();
        meta.insert(
            "reasoningEffort".into(),
            serde_json::Value::String(reasoning_effort.to_string()),
        );
        Self {
            session_id,
            model_id: model_id.to_string(),
            meta: Some(meta),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcResponse)]
struct LegacySetModelResponse {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcRequest)]
#[request(method = "_x.ai/exit_plan_mode", response = GrokExitPlanModeResponse)]
#[serde(rename_all = "camelCase")]
struct GrokExitPlanModeRequest {
    session_id: SessionId,
    #[serde(default)]
    tool_call_id: Option<String>,
    #[serde(default)]
    plan_content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
struct GrokExitPlanModeResponse {
    outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    feedback: Option<String>,
}

impl GrokExitPlanModeResponse {
    fn new(outcome: impl Into<String>) -> Self {
        Self {
            outcome: outcome.into(),
            feedback: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcRequest)]
#[request(method = "_x.ai/ask_user_question", response = GrokAskUserResponse)]
#[serde(rename_all = "camelCase")]
struct GrokAskUserRequest {
    session_id: SessionId,
    #[serde(default)]
    tool_call_id: Option<String>,
    #[serde(default)]
    questions: Vec<GrokQuestion>,
    #[serde(default)]
    mode: GrokAskUserMode,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum GrokAskUserMode {
    #[default]
    Default,
    Plan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GrokQuestion {
    question: String,
    #[serde(default)]
    multi_select: bool,
    #[serde(default)]
    options: Vec<GrokQuestionOption>,
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GrokQuestionOption {
    label: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    preview: Option<String>,
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Clone)]
struct GrokQuestionnaireContext {
    questions: Vec<GrokQuestion>,
    mode: GrokAskUserMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GrokQuestionAnnotation {
    #[serde(skip_serializing_if = "Option::is_none")]
    preview: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcResponse)]
#[serde(tag = "outcome", rename_all = "snake_case")]
enum GrokAskUserResponse {
    Accepted {
        answers: IndexMap<String, Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        annotations: Option<IndexMap<String, GrokQuestionAnnotation>>,
    },
    ChatAboutThis {
        #[serde(default)]
        partial_answers: IndexMap<String, String>,
    },
    SkipInterview {
        #[serde(default)]
        partial_answers: IndexMap<String, String>,
    },
    Cancelled,
}

impl GrokAskUserResponse {
    fn cancelled() -> Self {
        Self::Cancelled
    }

    fn from_submission(
        request: &GrokAskUserRequest,
        submission: &AcpQuestionnaireSubmission,
    ) -> Self {
        if submission.outcome == AcpQuestionnaireOutcome::Cancelled {
            return Self::Cancelled;
        }

        if submission.outcome != AcpQuestionnaireOutcome::Accepted {
            let partial_answers = questionnaire_partial_answers(&request.questions, submission);
            return match submission.outcome {
                AcpQuestionnaireOutcome::ChatAboutThis => Self::ChatAboutThis { partial_answers },
                AcpQuestionnaireOutcome::SkipInterview => Self::SkipInterview { partial_answers },
                AcpQuestionnaireOutcome::Accepted | AcpQuestionnaireOutcome::Cancelled => {
                    unreachable!("handled questionnaire outcome")
                }
            };
        }

        let mut answers = IndexMap::new();
        let mut annotations = IndexMap::new();
        let submitted = submission
            .answers
            .iter()
            .map(|answer| (answer.question_index, answer))
            .collect::<HashMap<_, _>>();
        for (question_index, question) in request.questions.iter().enumerate() {
            let Some(answer) = submitted.get(&question_index) else {
                continue;
            };
            let labels = selected_question_labels(question, answer);
            let notes = answer
                .other_text
                .as_ref()
                .filter(|text| !text.trim().is_empty())
                .cloned();
            if labels.is_empty() && notes.is_none() {
                continue;
            }
            answers.insert(
                question.question.clone(),
                if labels.is_empty() {
                    vec!["Other".into()]
                } else {
                    labels
                },
            );
            let preview = (!question.multi_select)
                .then(|| {
                    question
                        .options
                        .iter()
                        .enumerate()
                        .find(|(index, _)| answer.selected_option_indexes.contains(index))
                })
                .flatten()
                .and_then(|(_, option)| option.preview.clone());
            if preview.is_some() || notes.is_some() {
                annotations.insert(
                    question.question.clone(),
                    GrokQuestionAnnotation { preview, notes },
                );
            }
        }
        Self::Accepted {
            answers,
            annotations: (!annotations.is_empty()).then_some(annotations),
        }
    }
}

fn questionnaire_partial_answers(
    questions: &[GrokQuestion],
    submission: &AcpQuestionnaireSubmission,
) -> IndexMap<String, String> {
    let submitted = submission
        .answers
        .iter()
        .map(|answer| (answer.question_index, answer))
        .collect::<HashMap<_, _>>();
    questions
        .iter()
        .enumerate()
        .filter_map(|(question_index, question)| {
            let answer = submitted.get(&question_index)?;
            let labels = selected_question_labels(question, answer);
            // Grok's plan-only partial_answers wire type is a single string.
            // Preserve multi-select choices in their original option order via
            // an explicit AQBot compatibility convention instead of dropping
            // all but the first selection.
            let label = (!labels.is_empty())
                .then(|| labels.join(", "))
                .or_else(|| {
                    answer
                        .other_text
                        .as_deref()
                        .is_some_and(|text| !text.trim().is_empty())
                        .then(|| "Other".into())
                })?;
            Some((question.question.clone(), label))
        })
        .collect()
}

fn selected_question_labels(
    question: &GrokQuestion,
    answer: &AcpQuestionnaireAnswer,
) -> Vec<String> {
    question
        .options
        .iter()
        .enumerate()
        .filter(|(index, _)| answer.selected_option_indexes.contains(index))
        .map(|(_, option)| option.label.clone())
        .collect()
}

fn validate_questionnaire_submission(
    context: &GrokQuestionnaireContext,
    submission: &AcpQuestionnaireSubmission,
) -> Result<String, String> {
    if matches!(
        submission.outcome,
        AcpQuestionnaireOutcome::ChatAboutThis | AcpQuestionnaireOutcome::SkipInterview
    ) && context.mode != GrokAskUserMode::Plan
    {
        return Err("plan-only questionnaire action used outside plan mode".into());
    }
    if submission.outcome == AcpQuestionnaireOutcome::Cancelled {
        return Ok(String::new());
    }

    let mut seen_questions = HashSet::new();
    let mut summary = Vec::new();
    for answer in &submission.answers {
        let Some(question) = context.questions.get(answer.question_index) else {
            return Err(format!(
                "question index {} is out of range",
                answer.question_index
            ));
        };
        if !seen_questions.insert(answer.question_index) {
            return Err(format!(
                "question index {} was answered more than once",
                answer.question_index
            ));
        }
        let mut seen_options = HashSet::new();
        for option_index in &answer.selected_option_indexes {
            if question.options.get(*option_index).is_none() {
                return Err(format!(
                    "option index {option_index} is out of range for question {}",
                    answer.question_index
                ));
            }
            if !seen_options.insert(*option_index) {
                return Err(format!(
                    "option index {option_index} was selected more than once"
                ));
            }
        }
        let other_text = answer
            .other_text
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty());
        if !question.multi_select
            && (answer.selected_option_indexes.len() > 1
                || (!answer.selected_option_indexes.is_empty() && other_text.is_some()))
        {
            return Err(format!(
                "question {} only accepts one answer",
                answer.question_index
            ));
        }
        let mut labels = selected_question_labels(question, answer);
        if let Some(text) = other_text {
            labels.push(text.to_string());
        }
        if !labels.is_empty() {
            summary.push(format!("{}: {}", question.question, labels.join(", ")));
        }
    }
    Ok(summary.join("\n"))
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcRequest)]
#[request(method = "session/new", response = ExtendedNewSessionResponse)]
#[serde(rename_all = "camelCase")]
struct ExtendedNewSessionRequest {
    cwd: PathBuf,
    mcp_servers: Vec<McpServer>,
}

impl ExtendedNewSessionRequest {
    fn new(cwd: PathBuf) -> Self {
        Self {
            cwd,
            mcp_servers: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
struct ExtendedNewSessionResponse {
    /// Keep the official response as the source of truth. Its deserializer
    /// deliberately skips malformed or future config-option variants instead
    /// of rejecting the whole `session/new` response.
    #[serde(flatten)]
    standard: NewSessionResponse,
    #[serde(default)]
    models: Option<serde_json::Value>,
    #[serde(default)]
    reasoning_efforts: Option<serde_json::Value>,
}

fn aqbot_client_capabilities() -> ClientCapabilities {
    ClientCapabilities::new().session(ClientSessionCapabilities::new().config_options(
        SessionConfigOptionsCapabilities::new().boolean(BooleanConfigOptionCapabilities::new()),
    ))
}

async fn live_connection(live: &LiveSession) -> anyhow::Result<ConnectionTo<Agent>> {
    live.connection
        .lock()
        .await
        .clone()
        .ok_or_else(|| anyhow::anyhow!("ACP connection is not ready"))
}

async fn live_metadata(live: &LiveSession) -> anyhow::Result<AgentMetadata> {
    live.metadata
        .lock()
        .await
        .clone()
        .ok_or_else(|| anyhow::anyhow!("ACP agent metadata is not ready"))
}

fn snapshot_from_state(active: &ActiveSession, metadata: &AgentMetadata) -> AcpSessionSnapshot {
    AcpSessionSnapshot {
        session_id: active
            .id
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default(),
        modes: active.modes.clone(),
        config_options: active.config_options.clone(),
        agent_capabilities: metadata.capabilities.clone(),
    }
}

fn update_select_value(options: &mut [SessionConfigOption], config_id: &str, value: &str) {
    if let Some(option) = options
        .iter_mut()
        .find(|option| option.id.to_string() == config_id)
    {
        if let SessionConfigKind::Select(select) = &mut option.kind {
            select.current_value = value.to_string().into();
        }
    }
}

fn current_select_value(option: &SessionConfigOption) -> Option<String> {
    let SessionConfigKind::Select(select) = &option.kind else {
        return None;
    };
    Some(select.current_value.to_string())
}

fn current_config_value(option: &SessionConfigOption) -> Option<serde_json::Value> {
    match &option.kind {
        SessionConfigKind::Select(select) => {
            Some(serde_json::Value::String(select.current_value.to_string()))
        }
        SessionConfigKind::Boolean(boolean) => Some(serde_json::Value::Bool(boolean.current_value)),
        _ => None,
    }
}

fn restorable_config_selections(
    options: &[SessionConfigOption],
    replaced_config_id: &str,
) -> Vec<(String, serde_json::Value)> {
    options
        .iter()
        .filter(|option| option.id.to_string() != replaced_config_id)
        .filter(|option| !config_option_contains_plan(option))
        .filter(|option| {
            !option
                .meta
                .as_ref()
                .is_some_and(|meta| meta.contains_key("aqbotSpawnArg"))
        })
        .filter_map(|option| {
            current_config_value(option).map(|value| (option.id.to_string(), value))
        })
        .collect()
}

/// Encode the Agent's current plan/mode selection for thread persistence.
/// Standard modes keep their wire id for backward compatibility; config-backed
/// modes carry both the config id and value so they can be restored reliably.
pub fn persisted_mode_id(snapshot: &AcpSessionSnapshot) -> Option<String> {
    if let Some(modes) = snapshot.modes.as_ref() {
        return Some(modes.current_mode_id.to_string());
    }
    let option = snapshot
        .config_options
        .iter()
        .find(|option| config_option_contains_plan(option))?;
    let saved = PersistedConfigMode {
        config_id: option.id.to_string(),
        value: current_select_value(option)?,
    };
    Some(format!(
        "{PERSISTED_CONFIG_MODE_PREFIX}{}",
        serde_json::to_string(&saved).expect("string-only persisted mode is serializable")
    ))
}

fn send_grok_permission_mode(connection: &ConnectionTo<Agent>, mode: &str) -> anyhow::Result<()> {
    let payload = match mode {
        "default" => serde_json::json!({
            "permission_mode": "ask",
            "yolo_mode": false,
            "auto_mode": false,
        }),
        "auto" => serde_json::json!({
            "permission_mode": "auto",
            "yolo_mode": false,
            "auto_mode": true,
        }),
        "bypassPermissions" => serde_json::json!({
            "permission_mode": "always-approve",
            "yolo_mode": true,
            "auto_mode": false,
        }),
        _ => anyhow::bail!("unsupported Grok permission mode `{mode}`"),
    };
    let params = serde_json::value::to_raw_value(&payload)
        .map(Arc::from)
        .map_err(|error| anyhow::anyhow!("failed to encode Grok permission update: {error}"))?;
    connection
        .send_notification(ClientNotification::ExtNotification(ExtNotification::new(
            GROK_PERMISSION_SET_METHOD,
            params,
        )))
        .map_err(|error| anyhow::anyhow!("failed to update Grok permission mode: {error}"))
}

fn config_option_contains_value(option: &SessionConfigOption, expected: &str) -> bool {
    let SessionConfigKind::Select(select) = &option.kind else {
        return false;
    };
    match &select.options {
        SessionConfigSelectOptions::Ungrouped(options) => options
            .iter()
            .any(|option| option.value.to_string() == expected),
        SessionConfigSelectOptions::Grouped(groups) => groups.iter().any(|group| {
            group
                .options
                .iter()
                .any(|option| option.value.to_string() == expected)
        }),
        _ => false,
    }
}

fn sync_mode_config_values(options: &mut [SessionConfigOption], mode_id: &str) {
    for option in options.iter_mut().filter(|option| {
        option.category == Some(SessionConfigOptionCategory::Mode)
            && config_option_contains_plan(option)
            && config_option_contains_value(option, mode_id)
    }) {
        if let SessionConfigKind::Select(select) = &mut option.kind {
            select.current_value = mode_id.to_string().into();
        }
    }
}

fn sync_session_mode_from_config(
    active: &mut ActiveSession,
    option: &SessionConfigOption,
    mode_id: &str,
) {
    if option.category != Some(SessionConfigOptionCategory::Mode)
        || !config_option_contains_plan(option)
    {
        return;
    }
    let Some(modes) = active.modes.as_mut() else {
        return;
    };
    if modes
        .available_modes
        .iter()
        .any(|mode| mode.id.to_string() == mode_id)
    {
        modes.current_mode_id = SessionModeId::new(mode_id);
    }
}

fn apply_legacy_session_selection(
    options: &mut [SessionConfigOption],
    meta: Option<&agent_client_protocol::schema::v1::Meta>,
) {
    let Some(advertised) = meta
        .and_then(|meta| meta.get("x.ai/sessionConfig"))
        .and_then(|config| config.get("options"))
        .and_then(|options| options.as_array())
    else {
        return;
    };
    for selected in advertised.iter().filter(|option| {
        option
            .get("selected")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    }) {
        let Some(value) = selected.get("id").and_then(|value| value.as_str()) else {
            continue;
        };
        let target_id = match selected.get("category").and_then(|value| value.as_str()) {
            Some("model") => "model",
            Some("mode") => "reasoning_effort",
            _ => continue,
        };
        let is_known = options
            .iter()
            .find(|option| option.id.to_string() == target_id)
            .is_some_and(|option| validate_config_value(option, &serde_json::json!(value)).is_ok());
        if is_known {
            update_select_value(options, target_id, value);
        }
    }
}

fn agent_with_spawn_argument(
    agent: &ConfiguredAgent,
    flag: &str,
    value: &str,
) -> anyhow::Result<ConfiguredAgent> {
    if !["--model", "--reasoning-effort"].contains(&flag) {
        anyhow::bail!("unsupported ACP spawn option `{flag}`");
    }
    let value = value.trim();
    let use_agent_default = value == "__agent_default";
    if (!use_agent_default && value.is_empty())
        || value.len() > 64
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_.".contains(character))
    {
        anyhow::bail!("invalid ACP launch option value `{value}`");
    }

    let mut args = Vec::with_capacity(agent.args.len() + 2);
    let mut index = 0;
    while index < agent.args.len() {
        if agent.args[index] == flag {
            if index + 1 >= agent.args.len() {
                anyhow::bail!("ACP agent `{}` has `{flag}` without a value", agent.id);
            }
            index += 2;
        } else if agent.args[index].starts_with(&format!("{flag}=")) {
            index += 1;
        } else {
            args.push(agent.args[index].clone());
            index += 1;
        }
    }
    if use_agent_default {
        let mut updated = agent.clone();
        updated.args = args;
        return Ok(updated);
    }
    let transport_index = args
        .iter()
        .position(|argument| argument == "--acp")
        .or_else(|| args.iter().rposition(|argument| argument == "stdio"))
        .ok_or_else(|| anyhow::anyhow!("ACP agent `{}` has no ACP transport argument", agent.id))?;
    args.insert(transport_index, flag.to_string());
    args.insert(transport_index + 1, value.to_string());

    let mut updated = agent.clone();
    updated.args = args;
    Ok(updated)
}

pub fn configured_agent_with_reasoning_effort(
    agent: &ConfiguredAgent,
    effort: &str,
) -> anyhow::Result<ConfiguredAgent> {
    agent_with_spawn_argument(agent, "--reasoning-effort", effort)
}

pub fn configured_agent_with_model(
    agent: &ConfiguredAgent,
    model: &str,
) -> anyhow::Result<ConfiguredAgent> {
    agent_with_spawn_argument(agent, "--model", model)
}

fn launch_argument_value(agent: &ConfiguredAgent, flag: &str) -> Option<String> {
    agent.args.iter().enumerate().find_map(|(index, argument)| {
        if argument == flag {
            return agent.args.get(index + 1).cloned();
        }
        argument
            .strip_prefix(&format!("{flag}="))
            .map(str::to_string)
    })
}

fn copilot_probe_args(agent: &ConfiguredAgent, suffix: &[&str]) -> Vec<String> {
    let mut result = Vec::with_capacity(agent.args.len() + suffix.len());
    let mut index = 0;
    while index < agent.args.len() {
        let argument = &agent.args[index];
        if ["--model", "--reasoning-effort", "--effort"].contains(&argument.as_str()) {
            index += 2;
            continue;
        }
        if argument.starts_with("--model=")
            || argument.starts_with("--reasoning-effort=")
            || argument.starts_with("--effort=")
            || argument == "--acp"
            || argument == "--stdio"
        {
            index += 1;
            continue;
        }
        result.push(argument.clone());
        index += 1;
    }
    result.extend(suffix.iter().map(|argument| argument.to_string()));
    result
}

fn parse_copilot_models(help: &str) -> Vec<String> {
    let mut in_model_section = false;
    let mut models = Vec::new();
    for line in help.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("`model`:") {
            in_model_section = true;
            continue;
        }
        if !in_model_section {
            continue;
        }
        if trimmed.starts_with('`') {
            break;
        }
        let Some(quoted) = trimmed.strip_prefix("- \"") else {
            continue;
        };
        let Some(model) = quoted.strip_suffix('"') else {
            continue;
        };
        if !model.is_empty() && !models.iter().any(|known| known == model) {
            models.push(model.to_string());
        }
    }
    models
}

fn parse_copilot_reasoning_efforts(help: &str) -> Vec<String> {
    let flattened = help.split_whitespace().collect::<Vec<_>>().join(" ");
    let Some(flag_index) = flattened.find("--reasoning-effort") else {
        return Vec::new();
    };
    let remainder = &flattened[flag_index..];
    let Some(choice_index) = remainder.find("(choices:") else {
        return Vec::new();
    };
    let choices = &remainder[choice_index + "(choices:".len()..];
    let Some(end) = choices.find(')') else {
        return Vec::new();
    };
    choices[..end]
        .split(',')
        .map(|choice| choice.trim().trim_matches('"'))
        .filter(|choice| !choice.is_empty())
        .map(str::to_string)
        .collect()
}

async fn run_capability_probe(agent: &ConfiguredAgent, suffix: &[&str]) -> anyhow::Result<String> {
    let mut command = tokio::process::Command::new(&agent.command);
    command
        .args(copilot_probe_args(agent, suffix))
        .envs(agent.env.clone())
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(30), command.output())
        .await
        .map_err(|_| anyhow::anyhow!("ACP capability probe timed out"))??;
    if !output.status.success() {
        anyhow::bail!(
            "ACP capability probe failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    String::from_utf8(output.stdout)
        .map_err(|error| anyhow::anyhow!("ACP capability probe returned invalid UTF-8: {error}"))
}

async fn copilot_launch_catalog(agent: &ConfiguredAgent) -> anyhow::Result<LaunchOptionCatalog> {
    let key = format!(
        "{}\0{}",
        agent.command,
        copilot_probe_args(agent, &[]).join("\0")
    );
    let cache = LAUNCH_OPTION_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(cached) = cache.lock().await.get(&key).cloned() {
        return Ok(cached);
    }

    let (config_help, command_help) = tokio::try_join!(
        run_capability_probe(agent, &["help", "config"]),
        run_capability_probe(agent, &["--help"]),
    )?;
    let catalog = LaunchOptionCatalog {
        models: parse_copilot_models(&config_help),
        reasoning_efforts: parse_copilot_reasoning_efforts(&command_help),
    };
    if catalog.models.is_empty() || catalog.reasoning_efforts.is_empty() {
        anyhow::bail!(
            "GitHub Copilot capability discovery returned no {}",
            if catalog.models.is_empty() {
                "models"
            } else {
                "reasoning levels"
            }
        );
    }
    cache.lock().await.insert(key, catalog.clone());
    Ok(catalog)
}

fn launch_select_option(
    id: &str,
    name: &str,
    current: String,
    category: SessionConfigOptionCategory,
    flag: &str,
    values: &[String],
) -> SessionConfigOption {
    let mut choices = vec![
        SessionConfigSelectOption::new("__agent_default", "Agent default")
            .description("Use the agent's own configured default"),
    ];
    choices.extend(values.iter().map(|value| {
        SessionConfigSelectOption::new(
            value.clone(),
            if value == "auto" {
                "Auto".to_string()
            } else {
                value.clone()
            },
        )
    }));
    if !choices
        .iter()
        .any(|choice| choice.value.to_string() == current)
    {
        choices.push(SessionConfigSelectOption::new(
            current.clone(),
            current.clone(),
        ));
    }
    let mut marker = serde_json::Map::new();
    marker.insert(
        "aqbotSpawnArg".into(),
        serde_json::Value::String(flag.to_string()),
    );
    marker.insert(
        "aqbotCapabilitySource".into(),
        serde_json::Value::String("registry-cli".into()),
    );
    SessionConfigOption::select(
        id.to_string(),
        name.to_string(),
        current,
        SessionConfigSelectOptions::Ungrouped(choices),
    )
    .category(category)
    .meta(marker)
}

fn launch_live_model_option(current: String, values: &[String]) -> SessionConfigOption {
    let current = if current == "__agent_default" {
        values
            .iter()
            .find(|value| value.as_str() == "auto")
            .or_else(|| values.first())
            .cloned()
            .unwrap_or(current)
    } else {
        current
    };
    let mut option = launch_select_option(
        "model",
        "Model",
        current,
        SessionConfigOptionCategory::Model,
        "--model",
        values,
    );
    if let SessionConfigKind::Select(select) = &mut option.kind {
        if let SessionConfigSelectOptions::Ungrouped(choices) = &mut select.options {
            choices.retain(|choice| choice.value.to_string() != "__agent_default");
        }
    }
    let meta = option.meta.get_or_insert_with(Default::default);
    meta.remove("aqbotSpawnArg");
    meta.insert(
        "aqbotSetMethod".into(),
        serde_json::Value::String("session/set_model".into()),
    );
    option
}

async fn discover_launch_config_options(
    agent: &ConfiguredAgent,
) -> anyhow::Result<Vec<SessionConfigOption>> {
    let executable = std::path::Path::new(&agent.command)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(&agent.command)
        .to_ascii_lowercase();
    let is_copilot_acp = agent
        .args
        .iter()
        .any(|argument| argument.contains("@github/copilot"))
        || (executable == "copilot" && agent.args.iter().any(|argument| argument == "--acp"));
    if !is_copilot_acp {
        return Ok(Vec::new());
    }
    let catalog = copilot_launch_catalog(agent).await?;
    let mut models = vec!["auto".to_string()];
    models.extend(catalog.models);
    models.dedup();
    Ok(vec![
        launch_live_model_option(
            launch_argument_value(agent, "--model").unwrap_or_else(|| "__agent_default".into()),
            &models,
        ),
        launch_select_option(
            "reasoning_effort",
            "Reasoning",
            launch_argument_value(agent, "--reasoning-effort")
                .or_else(|| launch_argument_value(agent, "--effort"))
                .unwrap_or_else(|| "__agent_default".into()),
            SessionConfigOptionCategory::ThoughtLevel,
            "--reasoning-effort",
            &catalog.reasoning_efforts,
        ),
    ])
}

fn validate_config_value(
    option: &SessionConfigOption,
    value: &serde_json::Value,
) -> anyhow::Result<()> {
    match &option.kind {
        SessionConfigKind::Boolean(_) if value.is_boolean() => Ok(()),
        SessionConfigKind::Boolean(_) => {
            anyhow::bail!("config option `{}` requires a boolean", option.id)
        }
        SessionConfigKind::Select(select) => {
            let selected = value.as_str().ok_or_else(|| {
                anyhow::anyhow!("config option `{}` requires a string", option.id)
            })?;
            let exists = match &select.options {
                SessionConfigSelectOptions::Ungrouped(options) => options
                    .iter()
                    .any(|option| option.value.to_string() == selected),
                SessionConfigSelectOptions::Grouped(groups) => groups.iter().any(|group| {
                    group
                        .options
                        .iter()
                        .any(|option| option.value.to_string() == selected)
                }),
                _ => false,
            };
            if !exists {
                anyhow::bail!(
                    "unknown value `{selected}` for config option `{}`",
                    option.id
                );
            }
            Ok(())
        }
        _ => anyhow::bail!("unsupported config option type for `{}`", option.id),
    }
}

fn normalized_config_options(
    mut options: Vec<SessionConfigOption>,
    metadata: &AgentMetadata,
) -> Vec<SessionConfigOption> {
    // `aqbot*` metadata is host-reserved routing state. Never trust an Agent
    // supplied option to opt itself into process replacement or custom wire
    // methods; host-generated controls below add their markers afterwards.
    for option in &mut options {
        if let Some(meta) = option.meta.as_mut() {
            meta.retain(|key, _| !key.starts_with("aqbot"));
        }
    }
    if is_grok_shell(metadata)
        && !options
            .iter()
            .any(|option| is_agent_permission_config(option))
    {
        options.push(grok_permission_option("default"));
    }
    let has_model = options
        .iter()
        .any(|option| option.category == Some(SessionConfigOptionCategory::Model));
    if !has_model {
        if let Some(model) = legacy_model_option(metadata.meta.as_ref()) {
            options.push(model);
        }
    }
    let has_thought_level = options.iter().any(|option| {
        option.category == Some(SessionConfigOptionCategory::ThoughtLevel)
            || option.id.to_string() == "reasoning_effort"
    });
    if !has_thought_level {
        if let Some(reasoning) = legacy_reasoning_option(metadata.meta.as_ref()) {
            options.push(reasoning);
        }
    }
    for launch_option in &metadata.launch_config_options {
        let already_advertised = options.iter().any(|option| {
            option.id == launch_option.id
                || (launch_option.category.is_some() && option.category == launch_option.category)
        });
        if !already_advertised {
            options.push(launch_option.clone());
        }
    }
    options
}

fn normalized_config_options_for_session(
    options: Vec<SessionConfigOption>,
    metadata: &AgentMetadata,
    previous: &[SessionConfigOption],
) -> Vec<SessionConfigOption> {
    let mut normalized = normalized_config_options(options, metadata);
    for launch_option in &metadata.launch_config_options {
        let id = launch_option.id.to_string();
        let Some(previous_value) = previous
            .iter()
            .find(|option| option.id.to_string() == id)
            .and_then(current_config_value)
        else {
            continue;
        };
        let Some(option) = normalized
            .iter_mut()
            .find(|option| option.id.to_string() == id)
        else {
            continue;
        };
        if validate_config_value(option, &previous_value).is_err() {
            continue;
        }
        match (&mut option.kind, previous_value) {
            (SessionConfigKind::Select(select), serde_json::Value::String(value)) => {
                select.current_value = value.into();
            }
            (SessionConfigKind::Boolean(boolean), serde_json::Value::Bool(value)) => {
                boolean.current_value = value;
            }
            _ => {}
        }
    }
    normalized
}

fn is_grok_shell(metadata: &AgentMetadata) -> bool {
    metadata
        .meta
        .as_ref()
        .and_then(|meta| meta.get("grokShell"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn grok_permission_option(current: &str) -> SessionConfigOption {
    let mut marker = agent_client_protocol::schema::v1::Meta::new();
    marker.insert(
        "aqbotSetMethod".into(),
        serde_json::Value::String(GROK_PERMISSION_SET_METHOD.into()),
    );
    SessionConfigOption::select(
        GROK_PERMISSION_CONFIG_ID,
        "Permissions",
        current.to_string(),
        vec![
            SessionConfigSelectOption::new("default", "Ask")
                .description("Ask before protected tool calls"),
            SessionConfigSelectOption::new("auto", "Auto")
                .description("Use Grok's permission classifier"),
            SessionConfigSelectOption::new("bypassPermissions", "Always Approve")
                .description("Approve protected tool calls automatically"),
        ],
    )
    .category(SessionConfigOptionCategory::Other("permissions".into()))
    .meta(marker)
}

fn normalized_session_modes(
    modes: Option<SessionModeState>,
    metadata: &AgentMetadata,
) -> Option<SessionModeState> {
    modes.or_else(|| {
        is_grok_shell(metadata).then(|| {
            SessionModeState::new(
                "default",
                vec![
                    SessionMode::new("default", "Agent")
                        .description("Use Grok's normal coding mode"),
                    SessionMode::new("plan", "Plan")
                        .description("Create and review a plan without editing files"),
                ],
            )
        })
    })
}

fn legacy_model_option(
    meta: Option<&agent_client_protocol::schema::v1::Meta>,
) -> Option<SessionConfigOption> {
    let model_state = meta?.get("modelState")?;
    legacy_model_option_from_state(model_state)
}

fn legacy_model_option_from_state(model_state: &serde_json::Value) -> Option<SessionConfigOption> {
    let model_state = model_state.as_object()?;
    let current = model_state.get("currentModelId")?.as_str()?;
    let available = model_state.get("availableModels")?.as_array()?;
    let choices = available
        .iter()
        .filter_map(|model| {
            let id = model.get("modelId")?.as_str()?;
            let name = model
                .get("name")
                .and_then(|value| value.as_str())
                .unwrap_or(id);
            Some(SessionConfigSelectOption::new(
                id.to_string(),
                name.to_string(),
            ))
        })
        .collect::<Vec<_>>();
    if choices.is_empty() {
        return None;
    }
    let mut marker = serde_json::Map::new();
    marker.insert(
        "aqbotSetMethod".into(),
        serde_json::Value::String("session/set_model".into()),
    );
    Some(
        SessionConfigOption::select(
            "model",
            "Model",
            current.to_string(),
            SessionConfigSelectOptions::Ungrouped(choices),
        )
        .category(SessionConfigOptionCategory::Model)
        .meta(marker),
    )
}

fn legacy_reasoning_option(
    meta: Option<&agent_client_protocol::schema::v1::Meta>,
) -> Option<SessionConfigOption> {
    let model_state = meta?.get("modelState")?;
    legacy_reasoning_option_from_state(model_state)
}

fn legacy_reasoning_option_from_state(
    model_state: &serde_json::Value,
) -> Option<SessionConfigOption> {
    let model_state = model_state.as_object()?;
    let current_model = model_state.get("currentModelId")?.as_str()?;
    let model = model_state
        .get("availableModels")?
        .as_array()?
        .iter()
        .find(|model| {
            model.get("modelId").and_then(|value| value.as_str()) == Some(current_model)
        })?;
    let model_meta = model.get("_meta")?.as_object()?;
    let efforts = model_meta.get("reasoningEfforts")?.as_array()?;
    let current = model_meta
        .get("reasoningEffort")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .or_else(|| {
            efforts.iter().find_map(|effort| {
                effort
                    .get("default")
                    .and_then(|value| value.as_bool())
                    .filter(|is_default| *is_default)
                    .and_then(|_| {
                        effort
                            .get("value")
                            .or_else(|| effort.get("id"))
                            .and_then(|value| value.as_str())
                            .map(str::to_string)
                    })
            })
        })?;
    let choices = efforts
        .iter()
        .filter_map(|effort| {
            if let Some(value) = effort.as_str() {
                return Some(SessionConfigSelectOption::new(
                    value.to_string(),
                    value.to_string(),
                ));
            }
            let value = effort.get("value").or_else(|| effort.get("id"))?.as_str()?;
            let label = effort
                .get("label")
                .or_else(|| effort.get("name"))
                .and_then(|value| value.as_str())
                .unwrap_or(value);
            Some(
                SessionConfigSelectOption::new(value.to_string(), label.to_string()).description(
                    effort
                        .get("description")
                        .and_then(|value| value.as_str())
                        .map(str::to_string),
                ),
            )
        })
        .collect::<Vec<_>>();
    if choices.is_empty()
        || !choices
            .iter()
            .any(|choice| choice.value.to_string() == current)
    {
        return None;
    }
    let mut marker = serde_json::Map::new();
    marker.insert(
        "aqbotSetMethod".into(),
        serde_json::Value::String("session/set_model_reasoning".into()),
    );
    Some(
        SessionConfigOption::select(
            "reasoning_effort",
            "Reasoning",
            current,
            SessionConfigSelectOptions::Ungrouped(choices),
        )
        .category(SessionConfigOptionCategory::ThoughtLevel)
        .meta(marker),
    )
}

async fn prepare_live_session(
    live: &LiveSession,
    preferred_session_id: Option<&str>,
    event_tx: &mpsc::UnboundedSender<AcpEvent>,
) -> anyhow::Result<AcpSessionSnapshot> {
    let connection = live_connection(live).await?;
    let metadata = live_metadata(live).await?;
    let mut active = live.active.lock().await;
    let first_prepare = active.id.is_none();
    ensure_routed_agent_session(
        &connection,
        &live.cwd,
        preferred_session_id,
        &metadata,
        &mut active,
        event_tx,
        &live.routes,
        &live.session_open_lock,
        &live.route(),
    )
    .await?;
    if first_prepare && is_grok_shell(&metadata) {
        let permission_mode = if live.auto_approve.load(Ordering::Acquire) {
            "bypassPermissions"
        } else {
            "default"
        };
        update_select_value(
            &mut active.config_options,
            GROK_PERMISSION_CONFIG_ID,
            permission_mode,
        );
    }
    let snapshot = snapshot_from_state(&active, &metadata);
    let _ = event_tx.send(AcpEvent::SessionState {
        snapshot: snapshot.clone(),
    });
    Ok(snapshot)
}

#[allow(clippy::too_many_arguments)]
async fn ensure_routed_agent_session(
    connection: &ConnectionTo<Agent>,
    cwd: &PathBuf,
    preferred_session_id: Option<&str>,
    metadata: &AgentMetadata,
    active: &mut ActiveSession,
    event_tx: &mpsc::UnboundedSender<AcpEvent>,
    routes: &RouteMap,
    session_open_lock: &Arc<Mutex<()>>,
    route: &SessionRoute,
) -> anyhow::Result<()> {
    if let Some(session_id) = active.id.as_ref() {
        register_session_route(routes, session_id, route).await;
        return Ok(());
    }

    let _open = session_open_lock.lock().await;
    if let Some(session_id) = active.id.as_ref() {
        register_session_route(routes, session_id, route).await;
        return Ok(());
    }
    routes.lock().await.opening = Some(route.clone());
    let result = ensure_agent_session(
        connection,
        cwd,
        preferred_session_id,
        metadata,
        active,
        event_tx,
    )
    .await;
    match (&result, active.id.as_ref()) {
        (Ok(()), Some(session_id)) => register_session_route(routes, session_id, route).await,
        _ => {
            let mut routes = routes.lock().await;
            routes
                .by_session_id
                .retain(|_, existing| existing.permission_scope != route.permission_scope);
            if routes
                .opening
                .as_ref()
                .is_some_and(|opening| opening.permission_scope == route.permission_scope)
            {
                routes.opening = None;
            }
        }
    }
    result
}

async fn ensure_agent_session(
    connection: &ConnectionTo<Agent>,
    cwd: &PathBuf,
    preferred_session_id: Option<&str>,
    metadata: &AgentMetadata,
    active: &mut ActiveSession,
    event_tx: &mpsc::UnboundedSender<AcpEvent>,
) -> anyhow::Result<()> {
    if active.id.is_some() {
        return Ok(());
    }

    if let Some(preferred) = preferred_session_id {
        let preferred_id = SessionId::new(preferred);
        if metadata.capabilities.load_session {
            let _ = event_tx.send(AcpEvent::Status {
                message: ACP_STATUS_RESTORING_SESSION.into(),
            });
            match connection
                .send_request(LoadSessionRequest::new(preferred_id.clone(), cwd.clone()))
                .block_task()
                .await
            {
                Ok(response) => {
                    active.id = Some(preferred_id);
                    active.modes = normalized_session_modes(response.modes, metadata);
                    active.config_options = normalized_config_options(
                        response.config_options.unwrap_or_default(),
                        metadata,
                    );
                    apply_legacy_session_selection(
                        &mut active.config_options,
                        response.meta.as_ref(),
                    );
                    return Ok(());
                }
                Err(error) => {
                    let message = error.to_string();
                    if !is_missing_session_error(&message) {
                        return Err(anyhow::anyhow!("session/load failed: {message}"));
                    }
                    tracing::warn!(%error, session = preferred, "saved ACP session is missing");
                    let _ = event_tx.send(AcpEvent::Status {
                        message: ACP_STATUS_SAVED_SESSION_EXPIRED.into(),
                    });
                }
            }
        } else if metadata.capabilities.session_capabilities.resume.is_some() {
            match connection
                .send_request(ResumeSessionRequest::new(preferred_id.clone(), cwd.clone()))
                .block_task()
                .await
            {
                Ok(response) => {
                    active.id = Some(preferred_id);
                    active.modes = normalized_session_modes(response.modes, metadata);
                    active.config_options = normalized_config_options(
                        response.config_options.unwrap_or_default(),
                        metadata,
                    );
                    apply_legacy_session_selection(
                        &mut active.config_options,
                        response.meta.as_ref(),
                    );
                    return Ok(());
                }
                Err(error) => {
                    let message = error.to_string();
                    if !is_missing_session_error(&message) {
                        return Err(anyhow::anyhow!("session/resume failed: {message}"));
                    }
                    tracing::warn!(%error, session = preferred, "saved ACP session is missing");
                    let _ = event_tx.send(AcpEvent::Status {
                        message: ACP_STATUS_SAVED_SESSION_EXPIRED.into(),
                    });
                }
            }
        }
    }

    let _ = event_tx.send(AcpEvent::Status {
        message: ACP_STATUS_CREATING_SESSION.into(),
    });
    let response = connection
        .send_request(ExtendedNewSessionRequest::new(cwd.clone()))
        .block_task()
        .await
        .map_err(|error| anyhow::anyhow!("session/new failed: {error}"))?;
    let standard = response.standard;
    active.id = Some(standard.session_id);
    active.modes = normalized_session_modes(standard.modes, metadata);
    active.config_options =
        normalized_config_options(standard.config_options.unwrap_or_default(), metadata);
    apply_legacy_session_selection(&mut active.config_options, standard.meta.as_ref());
    if !active
        .config_options
        .iter()
        .any(|option| option.category == Some(SessionConfigOptionCategory::Model))
    {
        if let Some(model) = response
            .models
            .as_ref()
            .and_then(legacy_model_option_from_state)
            .or_else(|| legacy_model_option(standard.meta.as_ref()))
        {
            active.config_options.push(model);
        }
    }
    if let Some(reasoning_efforts) = response.reasoning_efforts.as_ref() {
        tracing::debug!(
            efforts = ?reasoning_efforts,
            "agent advertises spawn-time reasoning efforts without a live ACP config option"
        );
    }
    Ok(())
}

async fn run_one_prompt(
    connection: &ConnectionTo<Agent>,
    cwd: &PathBuf,
    prompt: &[ContentBlock],
    preferred_session_id: Option<&str>,
    active: &Arc<Mutex<ActiveSession>>,
    metadata: &Arc<Mutex<Option<AgentMetadata>>>,
    auto_approve: &Arc<AtomicBool>,
    event_tx: &mpsc::UnboundedSender<AcpEvent>,
    routes: &RouteMap,
    session_open_lock: &Arc<Mutex<()>>,
    route: &SessionRoute,
    prompt_state: &Arc<AtomicU8>,
    prompt_dispatch_lock: &Arc<Mutex<()>>,
) -> anyhow::Result<PromptOutcome> {
    let metadata = metadata
        .lock()
        .await
        .clone()
        .ok_or_else(|| anyhow::anyhow!("ACP agent metadata is not ready"))?;
    let mut session = active.lock().await;
    let session_open_result = ensure_routed_agent_session(
        connection,
        cwd,
        preferred_session_id,
        &metadata,
        &mut session,
        event_tx,
        routes,
        session_open_lock,
        route,
    )
    .await;
    if let Err(error) = session_open_result {
        if prompt_state.load(Ordering::Acquire) == PROMPT_CANCEL_REQUESTED {
            let snapshot = snapshot_from_state(&session, &metadata);
            return Ok(PromptOutcome {
                session_id: snapshot.session_id.clone(),
                stop_reason: "cancelled".into(),
                snapshot,
            });
        }
        return Err(error);
    }
    let grok_permission_mode = if is_grok_shell(&metadata) {
        Some(
            session
                .config_options
                .iter()
                .find(|option| option.id.to_string() == GROK_PERMISSION_CONFIG_ID)
                .and_then(current_select_value)
                .ok_or_else(|| anyhow::anyhow!("Grok permission mode is unavailable"))?,
        )
    } else {
        None
    };
    let mut snapshot = snapshot_from_state(&session, &metadata);
    if has_agent_permission_config(&session.config_options)
        || has_agent_permission_modes(session.modes.as_ref())
    {
        // Agent-advertised permission modes are authoritative. A global host
        // fallback must never turn Codex read-only/approval mode into auto-allow.
        auto_approve.store(false, Ordering::Release);
    }
    let _ = event_tx.send(AcpEvent::SessionState {
        snapshot: snapshot.clone(),
    });
    let mut session_id = session.id.clone().expect("session prepared above");
    drop(session);

    validate_prompt_content_blocks(prompt, &snapshot.agent_capabilities)?;
    let prompt_request = {
        let _dispatch = prompt_dispatch_lock.lock().await;
        match prompt_state.compare_exchange(
            PROMPT_QUEUED,
            PROMPT_RUNNING,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => {}
            Err(PROMPT_CANCEL_REQUESTED) => {
                return Ok(cancelled_prompt_outcome(&session_id, snapshot));
            }
            Err(state) => anyhow::bail!("invalid ACP prompt state `{state}` before dispatch"),
        }
        if let Some(permission_mode) = grok_permission_mode.as_deref() {
            send_grok_permission_mode(connection, permission_mode)?;
        }
        let _ = event_tx.send(AcpEvent::Status {
            message: ACP_STATUS_SENDING_PROMPT.into(),
        });
        connection.send_request(PromptRequest::new(session_id.clone(), prompt.to_vec()))
    };
    let prompt_result = prompt_request.block_task().await;

    if prompt_state.load(Ordering::Acquire) == PROMPT_CANCEL_REQUESTED {
        return Ok(cancelled_prompt_outcome(&session_id, snapshot));
    }

    let prompt_response = match prompt_result {
        Ok(r) => r,
        Err(e) => {
            let msg = e.to_string();
            if is_missing_session_error(&msg) {
                let _ = event_tx.send(AcpEvent::Status {
                    message: ACP_STATUS_SESSION_EXPIRED.into(),
                });
                let mut session = active.lock().await;
                *session = ActiveSession::default();
                ensure_routed_agent_session(
                    connection,
                    cwd,
                    None,
                    &metadata,
                    &mut session,
                    event_tx,
                    routes,
                    session_open_lock,
                    route,
                )
                .await?;
                session_id = session.id.clone().expect("session recreated above");
                snapshot = snapshot_from_state(&session, &metadata);
                let _ = event_tx.send(AcpEvent::SessionState {
                    snapshot: snapshot.clone(),
                });
                drop(session);
                let retry_request = {
                    let _dispatch = prompt_dispatch_lock.lock().await;
                    if prompt_state.load(Ordering::Acquire) == PROMPT_CANCEL_REQUESTED {
                        return Ok(cancelled_prompt_outcome(&session_id, snapshot));
                    }
                    connection.send_request(PromptRequest::new(session_id.clone(), prompt.to_vec()))
                };
                retry_request
                    .block_task()
                    .await
                    .map_err(|e2| anyhow::anyhow!("session/prompt failed: {e2}"))?
            } else {
                return Err(anyhow::anyhow!("session/prompt failed: {msg}"));
            }
        }
    };

    if prompt_state.load(Ordering::Acquire) == PROMPT_CANCEL_REQUESTED {
        return Ok(cancelled_prompt_outcome(&session_id, snapshot));
    }

    let stop_reason = format!("{:?}", prompt_response.stop_reason);
    let final_session = session_id.to_string();

    Ok(PromptOutcome {
        session_id: final_session,
        stop_reason,
        snapshot,
    })
}

fn cancelled_prompt_outcome(session_id: &SessionId, snapshot: AcpSessionSnapshot) -> PromptOutcome {
    PromptOutcome {
        session_id: session_id.to_string(),
        stop_reason: "cancelled".into(),
        snapshot,
    }
}

fn prompt_content_blocks(
    input: &AcpPromptInput,
    capabilities: &AgentCapabilities,
) -> anyhow::Result<Vec<ContentBlock>> {
    let mut blocks = Vec::with_capacity(1 + input.attachments.len());
    if !input.text.is_empty() {
        blocks.push(ContentBlock::Text(TextContent::new(input.text.clone())));
    }

    for attachment in &input.attachments {
        validate_prompt_attachment(attachment)?;
        if let Some(image_mime_type) = normalized_image_mime_type(attachment) {
            if !capabilities.prompt_capabilities.image {
                anyhow::bail!("ACP agent does not advertise image prompt capability");
            }
            let data = attachment
                .data
                .as_deref()
                .filter(|data| !data.is_empty())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "image attachment `{}` has no Base64 payload",
                        attachment.file_name
                    )
                })?;
            blocks.push(ContentBlock::Image(
                ImageContent::new(data, image_mime_type).uri(attachment.file_uri.clone()),
            ));
        } else {
            let size = i64::try_from(attachment.file_size).map_err(|_| {
                anyhow::anyhow!(
                    "attachment `{}` size exceeds the ACP ResourceLink limit",
                    attachment.file_name
                )
            })?;
            blocks.push(ContentBlock::ResourceLink(
                ResourceLink::new(attachment.file_name.clone(), attachment.file_uri.clone())
                    .mime_type(attachment.mime_type.clone())
                    .size(size),
            ));
        }
    }

    validate_prompt_content_blocks(&blocks, capabilities)?;
    Ok(blocks)
}

fn validate_prompt_content_blocks(
    blocks: &[ContentBlock],
    capabilities: &AgentCapabilities,
) -> anyhow::Result<()> {
    if blocks.is_empty() {
        anyhow::bail!("ACP prompt must contain text or an attachment");
    }
    for block in blocks {
        match block {
            ContentBlock::Image(_) if !capabilities.prompt_capabilities.image => {
                anyhow::bail!("ACP agent does not advertise image prompt capability");
            }
            ContentBlock::Audio(_) => {
                anyhow::bail!("AQBot ACP audio prompts are not supported");
            }
            ContentBlock::Resource(_) => {
                anyhow::bail!("AQBot ACP embedded resource prompts are not supported");
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_prompt_attachment(attachment: &AcpPromptAttachment) -> anyhow::Result<()> {
    if attachment.file_name.trim().is_empty() {
        anyhow::bail!("ACP attachment file name must not be empty");
    }
    if attachment.mime_type.trim().is_empty() {
        anyhow::bail!("ACP attachment MIME type must not be empty");
    }
    if attachment.file_uri.trim().is_empty() {
        anyhow::bail!(
            "ACP attachment `{}` file URI must not be empty",
            attachment.file_name
        );
    }
    Ok(())
}

fn is_image_mime_type(mime_type: &str) -> bool {
    mime_type
        .trim()
        .get(.."image/".len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("image/"))
}

fn normalized_image_mime_type(attachment: &AcpPromptAttachment) -> Option<String> {
    if is_image_mime_type(&attachment.mime_type) {
        return Some(attachment.mime_type.trim().to_ascii_lowercase());
    }
    let extension = std::path::Path::new(&attachment.file_name)
        .extension()
        .and_then(|value| value.to_str())?
        .to_ascii_lowercase();
    let mime_type = match extension.as_str() {
        "png" => "image/png",
        "apng" => "image/apng",
        "jpg" | "jpeg" | "jfif" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "heic" => "image/heic",
        "heif" => "image/heif",
        "tif" | "tiff" => "image/tiff",
        "jxl" => "image/jxl",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        _ => return None,
    };
    Some(mime_type.to_string())
}

fn is_missing_session_error(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    let known_phrase = [
        "session not found",
        "session_not_found",
        "unknown session",
        "no such session",
        "invalid session id",
    ]
    .iter()
    .any(|needle| message.contains(needle));
    let resource_not_found =
        message.contains("resource not found: session") && message.contains(" not found");
    known_phrase || resource_not_found
}

fn config_option_contains_plan(option: &SessionConfigOption) -> bool {
    let SessionConfigKind::Select(select) = &option.kind else {
        return false;
    };
    let values = match &select.options {
        SessionConfigSelectOptions::Ungrouped(options) => options
            .iter()
            .map(|option| option.value.to_string())
            .collect::<Vec<_>>(),
        SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|group| group.options.iter())
            .map(|option| option.value.to_string())
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    values.iter().any(|value| {
        value
            .rsplit(['#', '/', ':'])
            .next()
            .is_some_and(|token| token.eq_ignore_ascii_case("plan"))
    })
}

fn is_agent_permission_config(option: &SessionConfigOption) -> bool {
    if matches!(
        option.category.as_ref(),
        Some(SessionConfigOptionCategory::Other(category))
            if category.eq_ignore_ascii_case("permissions")
    ) {
        return true;
    }
    let identity = format!(
        "{} {} {}",
        option.id,
        option.name,
        option.description.as_deref().unwrap_or_default()
    )
    .to_ascii_lowercase();
    if ["permission", "approval", "allow_all", "allow-all", "access"]
        .iter()
        .any(|marker| identity.contains(marker))
    {
        return true;
    }
    option.category == Some(SessionConfigOptionCategory::Mode)
        && option.id.to_string() != "collaboration_mode"
        && !config_option_contains_plan(option)
}

fn has_agent_permission_config(options: &[SessionConfigOption]) -> bool {
    options.iter().any(is_agent_permission_config)
}

fn session_mode_token(value: &str) -> String {
    value
        .rsplit(['#', '/', ':'])
        .next()
        .unwrap_or(value)
        .chars()
        .filter(|character| !matches!(character, '-' | '_' | ' '))
        .flat_map(char::to_lowercase)
        .collect()
}

fn has_agent_permission_modes(modes: Option<&SessionModeState>) -> bool {
    let Some(modes) = modes else {
        return false;
    };
    let non_plan = modes
        .available_modes
        .iter()
        .filter(|mode| session_mode_token(&mode.id.to_string()) != "plan")
        .collect::<Vec<_>>();
    non_plan.len() >= 2
        && non_plan.iter().any(|mode| {
            matches!(
                session_mode_token(&mode.id.to_string()).as_str(),
                "acceptedits"
                    | "autoedit"
                    | "auto"
                    | "dontask"
                    | "bypasspermissions"
                    | "yolo"
                    | "unrestricted"
                    | "fullaccess"
                    | "readonly"
            )
        })
}

async fn handle_permission_request(
    request: RequestPermissionRequest,
    responder: Responder<RequestPermissionResponse>,
    auto: bool,
    permissions: PermissionMap,
    permission_scope: String,
    event_tx: Option<mpsc::UnboundedSender<AcpEvent>>,
    prompt_state: Arc<AtomicU8>,
    prompt_dispatch_lock: Arc<Mutex<()>>,
) -> Result<(), agent_client_protocol::Error> {
    let prompt_dispatch = prompt_dispatch_lock.lock().await;
    if prompt_state.load(Ordering::Acquire) == PROMPT_CANCEL_REQUESTED {
        responder.respond(RequestPermissionResponse::new(
            RequestPermissionOutcome::Cancelled,
        ))?;
        return Ok(());
    }
    if request.options.is_empty() {
        tracing::warn!("ACP agent sent a permission request without any response options");
        responder.respond(RequestPermissionResponse::new(
            RequestPermissionOutcome::Cancelled,
        ))?;
        return Ok(());
    }

    if auto {
        let option_id = request
            .options
            .iter()
            .find(|option| option.kind == PermissionOptionKind::AllowAlways)
            .or_else(|| {
                request
                    .options
                    .iter()
                    .find(|option| option.kind == PermissionOptionKind::AllowOnce)
            })
            .map(|option| option.option_id.clone());
        if let Some(id) = option_id {
            responder.respond(RequestPermissionResponse::new(
                RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(id)),
            ))?;
        } else {
            responder.respond(RequestPermissionResponse::new(
                RequestPermissionOutcome::Cancelled,
            ))?;
        }
        return Ok(());
    }

    let Some(event_tx) = event_tx else {
        responder.respond(RequestPermissionResponse::new(
            RequestPermissionOutcome::Cancelled,
        ))?;
        return Ok(());
    };

    let request_id = uuid::Uuid::new_v4().to_string();
    let options: Vec<PermissionOptionView> = request
        .options
        .iter()
        .map(|o| PermissionOptionView {
            option_id: o.option_id.to_string(),
            name: o.name.clone(),
            kind: Some(format!("{:?}", o.kind)),
            description: None,
        })
        .collect();

    let raw = serde_json::to_value(&request).map_err(|error| {
        agent_client_protocol::util::internal_error(format!(
            "failed to serialize permission request: {error}"
        ))
    })?;
    let tool_call_raw = raw
        .get("toolCall")
        .or_else(|| raw.get("tool_call"))
        .cloned()
        .unwrap_or_else(|| {
            serde_json::json!({
                "toolCallId": request.tool_call.tool_call_id.to_string(),
            })
        });
    let tool_call_id = request.tool_call.tool_call_id.to_string();
    let tool_kind = tool_call_raw
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let tool_status = tool_call_raw
        .get("status")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let (tx, rx) = oneshot::channel::<PermissionResolution>();
    {
        let mut map = permissions.lock().await;
        map.insert(
            request_id.clone(),
            PendingPermission {
                scope: permission_scope,
                interaction_kind: AcpInteractionKind::Permission,
                tool_call_id: Some(tool_call_id.clone()),
                options: options.clone(),
                questionnaire: None,
                event_tx: event_tx.clone(),
                sender: Some(tx),
                questionnaire_sender: None,
            },
        );
    }
    if event_tx
        .send(AcpEvent::ToolCall {
            tool_call_id: tool_call_id.clone(),
            title: request.tool_call.fields.title.clone(),
            kind: tool_kind,
            status: tool_status,
            raw: tool_call_raw,
        })
        .is_err()
        || event_tx
            .send(AcpEvent::PermissionRequest {
                request_id: request_id.clone(),
                interaction_kind: AcpInteractionKind::Permission,
                tool_call_id: Some(tool_call_id),
                title: request.tool_call.fields.title.clone(),
                raw,
                options: options.clone(),
            })
            .is_err()
    {
        permissions.lock().await.remove(&request_id);
        responder.respond(RequestPermissionResponse::new(
            RequestPermissionOutcome::Cancelled,
        ))?;
        return Ok(());
    }
    drop(prompt_dispatch);

    let selected = tokio::time::timeout(std::time::Duration::from_secs(600), rx).await;
    match selected {
        Ok(Ok(resolution)) => {
            responder.respond(RequestPermissionResponse::new(
                RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                    resolution.option_id,
                )),
            ))?;
        }
        _ => {
            expire_permission(&permissions, &request_id).await;
            responder.respond(RequestPermissionResponse::new(
                RequestPermissionOutcome::Cancelled,
            ))?;
        }
    }
    Ok(())
}

async fn handle_grok_exit_plan_mode(
    request: GrokExitPlanModeRequest,
    responder: Responder<GrokExitPlanModeResponse>,
    permissions: PermissionMap,
    permission_scope: String,
    event_tx: Option<mpsc::UnboundedSender<AcpEvent>>,
    prompt_state: Arc<AtomicU8>,
    prompt_dispatch_lock: Arc<Mutex<()>>,
) -> Result<(), agent_client_protocol::Error> {
    let prompt_dispatch = prompt_dispatch_lock.lock().await;
    if prompt_state.load(Ordering::Acquire) == PROMPT_CANCEL_REQUESTED {
        responder.respond(GrokExitPlanModeResponse::new("cancelled"))?;
        return Ok(());
    }
    let Some(event_tx) = event_tx else {
        responder.respond(GrokExitPlanModeResponse::new("cancelled"))?;
        return Ok(());
    };

    let request_id = uuid::Uuid::new_v4().to_string();
    let options = vec![
        PermissionOptionView {
            option_id: "approved".into(),
            name: "Approve and implement".into(),
            kind: Some("AllowOnce".into()),
            description: None,
        },
        PermissionOptionView {
            option_id: "cancelled".into(),
            name: "Continue planning".into(),
            kind: Some("RejectOnce".into()),
            description: None,
        },
        PermissionOptionView {
            option_id: "abandoned".into(),
            name: "Abandon plan".into(),
            kind: Some("RejectAlways".into()),
            description: None,
        },
    ];
    let mut raw = serde_json::to_value(&request).map_err(|error| {
        agent_client_protocol::util::internal_error(format!(
            "failed to serialize Grok plan review: {error}"
        ))
    })?;
    if let Some(object) = raw.as_object_mut() {
        object.insert(
            "kind".into(),
            serde_json::Value::String("plan_review".into()),
        );
        object.insert(
            "title".into(),
            serde_json::Value::String("Plan review".into()),
        );
    }
    let (tx, rx) = oneshot::channel::<PermissionResolution>();
    permissions.lock().await.insert(
        request_id.clone(),
        PendingPermission {
            scope: permission_scope,
            interaction_kind: AcpInteractionKind::PlanReview,
            tool_call_id: request.tool_call_id.clone(),
            options: options.clone(),
            questionnaire: None,
            event_tx: event_tx.clone(),
            sender: Some(tx),
            questionnaire_sender: None,
        },
    );
    // Do NOT emit AcpEvent::Plan here — that is reserved for structured
    // session/update plan todos. Plan-review documents would otherwise be
    // mis-parsed as the progress checklist (list lines from planContent).
    if event_tx
        .send(AcpEvent::PermissionRequest {
            request_id: request_id.clone(),
            interaction_kind: AcpInteractionKind::PlanReview,
            tool_call_id: request.tool_call_id.clone(),
            title: None,
            raw,
            options: options.clone(),
        })
        .is_err()
    {
        permissions.lock().await.remove(&request_id);
        responder.respond(GrokExitPlanModeResponse::new("cancelled"))?;
        return Ok(());
    }
    drop(prompt_dispatch);

    let selected = tokio::time::timeout(Duration::from_secs(600), rx).await;
    let resolution = match selected {
        Ok(Ok(resolution))
            if ["approved", "cancelled", "abandoned"].contains(&resolution.option_id.as_str()) =>
        {
            resolution
        }
        _ => {
            expire_permission(&permissions, &request_id).await;
            PermissionResolution {
                option_id: "cancelled".into(),
                feedback: None,
            }
        }
    };
    responder.respond(GrokExitPlanModeResponse {
        outcome: resolution.option_id,
        feedback: resolution.feedback,
    })?;
    Ok(())
}

async fn handle_grok_ask_user(
    request: GrokAskUserRequest,
    responder: Responder<GrokAskUserResponse>,
    permissions: PermissionMap,
    permission_scope: String,
    event_tx: Option<mpsc::UnboundedSender<AcpEvent>>,
    prompt_state: Arc<AtomicU8>,
    prompt_dispatch_lock: Arc<Mutex<()>>,
) -> Result<(), agent_client_protocol::Error> {
    let prompt_dispatch = prompt_dispatch_lock.lock().await;
    if prompt_state.load(Ordering::Acquire) == PROMPT_CANCEL_REQUESTED {
        responder.respond(GrokAskUserResponse::cancelled())?;
        return Ok(());
    }
    let Some(event_tx) = event_tx else {
        responder.respond(GrokAskUserResponse::cancelled())?;
        return Ok(());
    };
    let Some(first_question) = request.questions.first() else {
        tracing::warn!("Grok sent an empty ask_user_question questionnaire");
        responder.respond(GrokAskUserResponse::cancelled())?;
        return Ok(());
    };

    let request_id = uuid::Uuid::new_v4().to_string();
    let options = request
        .questions
        .iter()
        .enumerate()
        .flat_map(|(question_index, question)| {
            question
                .options
                .iter()
                .enumerate()
                .map(move |(option_index, option)| PermissionOptionView {
                    option_id: format!("answer:{question_index}:{option_index}"),
                    name: option.label.clone(),
                    kind: Some("AllowOnce".into()),
                    description: option.description.clone(),
                })
        })
        .collect::<Vec<_>>();
    let mut raw = serde_json::to_value(&request).map_err(|error| {
        agent_client_protocol::util::internal_error(format!(
            "failed to serialize Grok user question: {error}"
        ))
    })?;
    if let Some(object) = raw.as_object_mut() {
        object.insert(
            "kind".into(),
            serde_json::Value::String("ask_user_question".into()),
        );
        object.insert(
            "title".into(),
            serde_json::Value::String(first_question.question.clone()),
        );
    }
    let (tx, rx) = oneshot::channel::<AcpQuestionnaireSubmission>();
    permissions.lock().await.insert(
        request_id.clone(),
        PendingPermission {
            scope: permission_scope,
            interaction_kind: AcpInteractionKind::Question,
            tool_call_id: request.tool_call_id.clone(),
            options: options.clone(),
            questionnaire: Some(GrokQuestionnaireContext {
                questions: request.questions.clone(),
                mode: request.mode,
            }),
            event_tx: event_tx.clone(),
            sender: None,
            questionnaire_sender: Some(tx),
        },
    );
    if event_tx
        .send(AcpEvent::PermissionRequest {
            request_id: request_id.clone(),
            interaction_kind: AcpInteractionKind::Question,
            tool_call_id: request.tool_call_id.clone(),
            title: Some(first_question.question.clone()),
            raw,
            options: options.clone(),
        })
        .is_err()
    {
        permissions.lock().await.remove(&request_id);
        responder.respond(GrokAskUserResponse::cancelled())?;
        return Ok(());
    }
    drop(prompt_dispatch);

    let selected = tokio::time::timeout(Duration::from_secs(600), rx).await;
    let response = match selected {
        Ok(Ok(submission)) => GrokAskUserResponse::from_submission(&request, &submission),
        _ => {
            expire_permission(&permissions, &request_id).await;
            GrokAskUserResponse::cancelled()
        }
    };
    responder.respond(response)?;
    Ok(())
}

async fn resolve_session_route(routes: &RouteMap, session_id: &SessionId) -> Option<SessionRoute> {
    let session_id = session_id.to_string();
    let mut routes = routes.lock().await;
    if let Some(route) = routes.by_session_id.get(&session_id) {
        return Some(route.clone());
    }
    let route = routes.opening.clone()?;
    routes.by_session_id.insert(session_id, route.clone());
    Some(route)
}

async fn register_session_route(routes: &RouteMap, session_id: &SessionId, route: &SessionRoute) {
    let mut routes = routes.lock().await;
    routes
        .by_session_id
        .retain(|_, existing| existing.permission_scope != route.permission_scope);
    routes
        .by_session_id
        .insert(session_id.to_string(), route.clone());
    if routes
        .opening
        .as_ref()
        .is_some_and(|opening| opening.permission_scope == route.permission_scope)
    {
        routes.opening = None;
    }
}

async fn route_session_notification(
    notification: SessionNotification,
    routes: &RouteMap,
    metadata: &Arc<Mutex<Option<AgentMetadata>>>,
) {
    let Some(route) = resolve_session_route(routes, &notification.session_id).await else {
        tracing::warn!(
            session_id = %notification.session_id,
            "ignoring ACP update for an unknown logical session"
        );
        return;
    };
    let event_tx = route.event_slot.lock().await.clone();
    let (discard_tx, _discard_rx) = mpsc::unbounded_channel();
    map_session_notification(
        &notification,
        event_tx.as_ref().unwrap_or(&discard_tx),
        &route.active,
        metadata,
    )
    .await;
}

fn agent_options_for_launch_refresh(previous: &[SessionConfigOption]) -> Vec<SessionConfigOption> {
    previous
        .iter()
        .filter(|option| {
            !option
                .meta
                .as_ref()
                .is_some_and(|meta| meta.contains_key("aqbotSpawnArg"))
        })
        .cloned()
        .collect()
}

async fn refresh_routed_config_options(routes: &RouteMap, metadata: &AgentMetadata) {
    let mut seen = HashSet::new();
    let routed = routes
        .lock()
        .await
        .by_session_id
        .values()
        .filter(|route| seen.insert(route.permission_scope.clone()))
        .cloned()
        .collect::<Vec<_>>();
    for route in routed {
        let mut active = route.active.lock().await;
        let previous = active.config_options.clone();
        let agent_options = agent_options_for_launch_refresh(&previous);
        active.config_options =
            normalized_config_options_for_session(agent_options, metadata, &previous);
        if active.id.is_some() {
            if let Some(event_tx) = route.event_slot.lock().await.clone() {
                let _ = event_tx.send(AcpEvent::SessionState {
                    snapshot: snapshot_from_state(&active, metadata),
                });
            }
        }
    }
}

#[derive(Serialize)]
struct GrokRetryStatusPayload<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    attempt: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    maximum: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<&'a str>,
}

fn grok_retry_status(notification: &ExtNotification) -> Option<(SessionId, String)> {
    let method = notification.method.trim_start_matches('_');
    if !matches!(method, "x.ai/session/update" | "x.ai/session_notification") {
        return None;
    }
    let params: serde_json::Value = serde_json::from_str(notification.params.get()).ok()?;
    let session_id = params
        .get("sessionId")
        .or_else(|| params.get("session_id"))?
        .as_str()?;
    let update = params.get("update").unwrap_or(&params);
    let kind = update
        .get("sessionUpdate")
        .or_else(|| update.get("session_update"))?
        .as_str()?;
    if kind != "retry_state" {
        return None;
    }
    let attempt = update.get("attempt").and_then(serde_json::Value::as_u64);
    let maximum = update
        .get("maxRetries")
        .or_else(|| update.get("max_retries"))
        .and_then(serde_json::Value::as_u64);
    let detail = update
        .get("reason")
        .or_else(|| update.get("status"))
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty());
    let payload = GrokRetryStatusPayload {
        attempt,
        maximum,
        detail,
    };
    let message = format!(
        "{ACP_STATUS_GROK_RETRY_PREFIX}{}",
        serde_json::to_string(&payload).ok()?
    );
    Some((SessionId::new(session_id), message))
}

async fn route_extension_notification(notification: ExtNotification, routes: &RouteMap) {
    let Some((session_id, message)) = grok_retry_status(&notification) else {
        tracing::debug!(method = %notification.method, "ignoring unsupported ACP extension notification");
        return;
    };
    let Some(route) = resolve_session_route(routes, &session_id).await else {
        tracing::warn!(%session_id, "ignoring Grok retry update for an unknown logical session");
        return;
    };
    let event_tx = route.event_slot.lock().await.clone();
    if let Some(event_tx) = event_tx {
        let _ = event_tx.send(AcpEvent::Status { message });
    }
}

async fn map_session_notification(
    notification: &SessionNotification,
    event_tx: &mpsc::UnboundedSender<AcpEvent>,
    active: &Arc<Mutex<ActiveSession>>,
    metadata: &Arc<Mutex<Option<AgentMetadata>>>,
) {
    let update = &notification.update;
    let value = match serde_json::to_value(update) {
        Ok(v) => v,
        Err(error) => {
            tracing::warn!(%error, "failed to serialize ACP session notification");
            return;
        }
    };

    let kind = value
        .get("sessionUpdate")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    match &notification.update {
        SessionUpdate::CurrentModeUpdate(update) => {
            let mut active = active.lock().await;
            if let Some(modes) = active.modes.as_mut() {
                modes.current_mode_id = update.current_mode_id.clone();
            }
            sync_mode_config_values(
                &mut active.config_options,
                &update.current_mode_id.to_string(),
            );
            if let Some(metadata) = metadata.lock().await.clone() {
                let _ = event_tx.send(AcpEvent::SessionState {
                    snapshot: snapshot_from_state(&active, &metadata),
                });
            }
            return;
        }
        SessionUpdate::ConfigOptionUpdate(update) => {
            let mut active = active.lock().await;
            if let Some(metadata) = metadata.lock().await.clone() {
                let previous = active.config_options.clone();
                active.config_options = normalized_config_options_for_session(
                    update.config_options.clone(),
                    &metadata,
                    &previous,
                );
                let _ = event_tx.send(AcpEvent::SessionState {
                    snapshot: snapshot_from_state(&active, &metadata),
                });
            }
            return;
        }
        _ => {}
    }

    match kind {
        kind if is_assistant_message_update(kind) => {
            if let Some(text) = extract_text_content(&value) {
                let _ = event_tx.send(AcpEvent::StreamText { text });
            }
        }
        "user_message_chunk" => {
            tracing::debug!("ignoring ACP user-message echo in assistant stream");
        }
        "agent_thought_chunk" => {
            if let Some(text) = extract_text_content(&value) {
                let _ = event_tx.send(AcpEvent::StreamThinking { thinking: text });
            }
        }
        "tool_call" => {
            let tool_call_id = value
                .get("toolCallId")
                .or_else(|| value.get("tool_call_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let title = value
                .get("title")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let kind = value
                .get("kind")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let status = value
                .get("status")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let _ = event_tx.send(AcpEvent::ToolCall {
                tool_call_id,
                title,
                kind,
                status,
                raw: value,
            });
        }
        "tool_call_update" => {
            let tool_call_id = value
                .get("toolCallId")
                .or_else(|| value.get("tool_call_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let status = value
                .get("status")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let _ = event_tx.send(AcpEvent::ToolCallUpdate {
                tool_call_id,
                status,
                raw: value,
            });
        }
        "plan" => {
            let _ = event_tx.send(AcpEvent::Plan { raw: value });
        }
        _ => {
            tracing::debug!(%kind, "acp session update");
        }
    }
}

fn is_assistant_message_update(kind: &str) -> bool {
    kind == "agent_message_chunk"
}

fn extract_text_content(value: &serde_json::Value) -> Option<String> {
    if let Some(c) = value.get("content") {
        if let Some(t) = c.get("text").and_then(|v| v.as_str()) {
            return Some(t.to_string());
        }
        if let Some(t) = c.as_str() {
            return Some(t.to_string());
        }
    }
    value
        .get("text")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::PromptCapabilities;

    fn pending_permission(
        scope: &str,
        event_tx: mpsc::UnboundedSender<AcpEvent>,
    ) -> (PendingPermission, oneshot::Receiver<PermissionResolution>) {
        let (sender, receiver) = oneshot::channel();
        (
            PendingPermission {
                scope: scope.into(),
                interaction_kind: AcpInteractionKind::Permission,
                tool_call_id: Some("tool-1".into()),
                options: vec![PermissionOptionView {
                    option_id: "allow-once".into(),
                    name: "Allow once".into(),
                    kind: Some("AllowOnce".into()),
                    description: None,
                }],
                questionnaire: None,
                event_tx,
                sender: Some(sender),
                questionnaire_sender: None,
            },
            receiver,
        )
    }

    fn pending_questionnaire(
        scope: &str,
        event_tx: mpsc::UnboundedSender<AcpEvent>,
    ) -> (
        PendingPermission,
        oneshot::Receiver<AcpQuestionnaireSubmission>,
    ) {
        let (sender, receiver) = oneshot::channel();
        (
            PendingPermission {
                scope: scope.into(),
                interaction_kind: AcpInteractionKind::Question,
                tool_call_id: Some("question-tool-1".into()),
                options: vec![],
                questionnaire: Some(GrokQuestionnaireContext {
                    questions: vec![GrokQuestion {
                        question: "Which layers?".into(),
                        multi_select: true,
                        options: vec![GrokQuestionOption {
                            label: "Frontend".into(),
                            description: None,
                            preview: None,
                            id: Some("ui".into()),
                        }],
                        id: Some("layers".into()),
                    }],
                    mode: GrokAskUserMode::Default,
                }),
                event_tx,
                sender: None,
                questionnaire_sender: Some(sender),
            },
            receiver,
        )
    }

    #[tokio::test]
    async fn resolving_permission_emits_one_selected_terminal_event() {
        let runtime = AcpRuntime::new();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let (pending, selected_rx) = pending_permission("scope-1", event_tx);
        runtime
            .permissions
            .lock()
            .await
            .insert("request-1".into(), pending);

        assert!(
            runtime
                .resolve_permission("request-1", "allow-once".into(), None)
                .await
        );
        let resolution = selected_rx.await.expect("selected option");
        assert_eq!(resolution.option_id, "allow-once");
        assert_eq!(resolution.feedback, None);
        assert!(matches!(
            event_rx.recv().await,
            Some(AcpEvent::InteractionClosed {
                request_id,
                interaction_kind: AcpInteractionKind::Permission,
                tool_call_id: Some(tool_call_id),
                outcome: AcpInteractionOutcome::Selected,
                selected_option_id: Some(option_id),
                selected_option_kind: Some(option_kind),
                selected_option_name: Some(option_name),
            }) if request_id == "request-1"
                && tool_call_id == "tool-1"
                && option_id == "allow-once"
                && option_kind == "AllowOnce"
                && option_name == "Allow once"
        ));
        assert!(event_rx.try_recv().is_err());
        assert!(
            !runtime
                .resolve_permission("request-1", "allow-once".into(), None)
                .await
        );
        assert!(event_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn resolving_questionnaire_emits_one_selected_terminal_event() {
        let runtime = AcpRuntime::new();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let (pending, response_rx) = pending_questionnaire("scope-1", event_tx);
        runtime
            .permissions
            .lock()
            .await
            .insert("questionnaire-1".into(), pending);
        let submission = AcpQuestionnaireSubmission {
            outcome: AcpQuestionnaireOutcome::Accepted,
            answers: vec![AcpQuestionnaireAnswer {
                question_index: 0,
                selected_option_indexes: vec![0],
                other_text: None,
            }],
        };

        let summary = runtime
            .resolve_questionnaire("questionnaire-1", submission.clone())
            .await
            .expect("resolve questionnaire");

        assert_eq!(summary, "Which layers?: Frontend");
        assert_eq!(
            response_rx.await.expect("questionnaire response"),
            submission
        );
        assert!(matches!(
            event_rx.recv().await,
            Some(AcpEvent::InteractionClosed {
                request_id,
                interaction_kind: AcpInteractionKind::Question,
                tool_call_id: Some(tool_call_id),
                outcome: AcpInteractionOutcome::Selected,
                selected_option_id: Some(option_id),
                selected_option_name: Some(option_name),
                ..
            }) if request_id == "questionnaire-1"
                && tool_call_id == "question-tool-1"
                && option_id == "accepted"
                && option_name == "Which layers?: Frontend"
        ));
        assert!(event_rx.try_recv().is_err());
        assert!(runtime
            .resolve_questionnaire("questionnaire-1", submission)
            .await
            .is_err());
        assert!(event_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn resolving_empty_plan_questionnaire_preserves_the_selected_action() {
        let runtime = AcpRuntime::new();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let (mut pending, response_rx) = pending_questionnaire("scope-1", event_tx);
        pending.interaction_kind = AcpInteractionKind::PlanReview;
        pending
            .questionnaire
            .as_mut()
            .expect("questionnaire context")
            .mode = GrokAskUserMode::Plan;
        runtime
            .permissions
            .lock()
            .await
            .insert("questionnaire-1".into(), pending);
        let submission = AcpQuestionnaireSubmission {
            outcome: AcpQuestionnaireOutcome::SkipInterview,
            answers: vec![],
        };

        let summary = runtime
            .resolve_questionnaire("questionnaire-1", submission.clone())
            .await
            .expect("resolve empty plan questionnaire");

        assert!(summary.is_empty());
        assert_eq!(
            response_rx.await.expect("questionnaire response"),
            submission
        );
        assert!(matches!(
            event_rx.recv().await,
            Some(AcpEvent::InteractionClosed {
                interaction_kind: AcpInteractionKind::PlanReview,
                outcome: AcpInteractionOutcome::Selected,
                selected_option_id: Some(option_id),
                selected_option_name: Some(option_name),
                ..
            }) if option_id == "skip_interview" && option_name.is_empty()
        ));
        assert!(event_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn cancelling_scope_emits_one_cancelled_event_only_for_that_scope() {
        let runtime = AcpRuntime::new();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let (first, _first_rx) = pending_permission("scope-1", event_tx.clone());
        let (second, _second_rx) = pending_permission("scope-2", event_tx);
        let mut permissions = runtime.permissions.lock().await;
        permissions.insert("request-1".into(), first);
        permissions.insert("request-2".into(), second);
        drop(permissions);

        runtime.cancel_permissions("scope-1").await;

        assert!(matches!(
            event_rx.recv().await,
            Some(AcpEvent::InteractionClosed {
                request_id,
                outcome: AcpInteractionOutcome::Cancelled,
                selected_option_id: None,
                ..
            }) if request_id == "request-1"
        ));
        assert!(event_rx.try_recv().is_err());
        assert!(runtime.permissions.lock().await.contains_key("request-2"));
        runtime.cancel_permissions("scope-1").await;
        assert!(event_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn expiring_permission_emits_one_expired_terminal_event() {
        let permissions = Arc::new(Mutex::new(HashMap::new()));
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let (pending, _selected_rx) = pending_permission("scope-1", event_tx);
        permissions.lock().await.insert("request-1".into(), pending);

        expire_permission(&permissions, "request-1").await;
        expire_permission(&permissions, "request-1").await;

        assert!(matches!(
            event_rx.recv().await,
            Some(AcpEvent::InteractionClosed {
                request_id,
                outcome: AcpInteractionOutcome::Expired,
                selected_option_id: None,
                ..
            }) if request_id == "request-1"
        ));
        assert!(event_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn timeout_wins_a_resolution_race_without_losing_the_terminal_event() {
        let runtime = AcpRuntime::new();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let (pending, selected_rx) = pending_permission("scope-1", event_tx);
        drop(selected_rx);
        runtime
            .permissions
            .lock()
            .await
            .insert("request-1".into(), pending);

        assert!(
            !runtime
                .resolve_permission("request-1", "allow-once".into(), None)
                .await
        );
        expire_permission(&runtime.permissions, "request-1").await;

        assert!(matches!(
            event_rx.recv().await,
            Some(AcpEvent::InteractionClosed {
                outcome: AcpInteractionOutcome::Expired,
                ..
            })
        ));
        assert!(event_rx.try_recv().is_err());
    }

    #[test]
    fn interaction_closed_serializes_without_raw_payload_inference() {
        let value = serde_json::to_value(AcpEvent::InteractionClosed {
            request_id: "request-1".into(),
            interaction_kind: AcpInteractionKind::Permission,
            tool_call_id: Some("tool-1".into()),
            outcome: AcpInteractionOutcome::Selected,
            selected_option_id: Some("allow-once".into()),
            selected_option_kind: Some("AllowOnce".into()),
            selected_option_name: Some("Allow once".into()),
        })
        .expect("serialize terminal event");

        assert_eq!(value["type"], "interactionClosed");
        assert_eq!(value["interactionKind"], "permission");
        assert_eq!(value["outcome"], "selected");
        assert_eq!(value["selectedOptionId"], "allow-once");
        assert!(value.get("raw").is_none());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn login_shell_path_reaches_a_bare_acp_process_command() {
        use std::os::unix::fs::PermissionsExt;

        let directory =
            std::env::temp_dir().join(format!("aqbot-acp-shell-path-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).expect("create fake Agent bin directory");
        let command = format!("aqbot-path-agent-{}", uuid::Uuid::new_v4());
        let executable = directory.join(&command);
        std::fs::write(&executable, "#!/bin/sh\nexit 0\n").expect("write fake Agent");
        let mut permissions = std::fs::metadata(&executable)
            .expect("read fake Agent permissions")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&executable, permissions).expect("make fake Agent executable");
        let agent = ConfiguredAgent {
            id: "path-agent".into(),
            name: "PATH Agent".into(),
            enabled: true,
            source: "custom".into(),
            command,
            args: Vec::new(),
            env: HashMap::new(),
            icon: None,
            sort: 0,
        };
        let process_agent =
            configured_agent_for_process_with_path(&agent, directory.to_string_lossy().as_ref());

        let (_, _, _, mut child) = build_acp_agent(&process_agent)
            .spawn_process()
            .expect("login-shell PATH must resolve the bare Agent command");
        let status = child.status().await.expect("wait for fake Agent");

        std::fs::remove_dir_all(&directory).expect("remove fake Agent directory");
        assert!(status.success());
        assert!(agent.env.is_empty(), "runtime PATH must not be persisted");
    }

    #[test]
    fn structured_dependency_errors_are_sanitized_without_unwrapping_business_data() {
        let raw = concat!(
            "Internal error: ",
            r#"{"spawned_at":"/Users/runner/.cargo/registry/src/agent-client-protocol/src/jsonrpc.rs:1732:39","data":{"kind":"spawn","data":"missing runtime"}}"#
        );

        let error = summarize_agent_spawn_error(raw, "npx");

        assert!(
            error.contains(r#""kind":"spawn""#),
            "missing structured data: {error}"
        );
        assert!(
            error.contains(r#""data":"missing runtime""#),
            "ordinary data field was unwrapped: {error}"
        );
        assert!(
            !error.contains("spawned_at")
                && !error.contains("/Users/runner")
                && !error.contains("jsonrpc.rs"),
            "dependency build path leaked into the user-facing error: {error}"
        );
    }

    #[test]
    fn null_dependency_error_data_does_not_leak_its_spawn_location() {
        let raw = concat!(
            "Internal error: ",
            r#"{"spawned_at":"/Users/runner/.cargo/registry/src/agent-client-protocol/src/jsonrpc.rs:1732:39","data":null}"#
        );

        let error = summarize_agent_spawn_error(raw, "npx");

        assert_eq!(error, "null");
        assert!(!error.contains("spawned_at") && !error.contains("/Users/runner"));
    }

    #[test]
    fn ordinary_json_data_is_not_treated_as_a_dependency_wrapper() {
        let raw = r#"Internal error: {"data":"business reason","code":42}"#;

        let error = summarize_agent_spawn_error(raw, "npx");

        assert!(error.contains(r#""data":"business reason""#));
        assert!(error.contains(r#""code":42"#));
    }

    #[tokio::test]
    async fn missing_agent_executable_reports_the_command_without_dependency_source_paths() {
        let runtime = AcpRuntime::new();
        let command = format!("aqbot-missing-acp-agent-{}", uuid::Uuid::new_v4());
        let agent = ConfiguredAgent {
            id: "missing-agent".into(),
            name: "Missing Agent".into(),
            enabled: true,
            source: "custom".into(),
            command: command.clone(),
            args: Vec::new(),
            env: HashMap::new(),
            icon: None,
            sort: 0,
        };

        let error = runtime
            .prewarm_agent(&agent, false, RuntimeLimits::new(60, 1))
            .await
            .expect_err("a missing ACP executable must fail startup")
            .to_string();

        assert!(error.contains(&command), "missing launch command: {error}");
        assert!(
            error.to_ascii_lowercase().contains("os error 2"),
            "missing operating-system reason: {error}"
        );
        assert!(
            !error.contains("agent-client-protocol") && !error.contains("jsonrpc.rs"),
            "dependency build path leaked into the user-facing error: {error}"
        );
    }

    #[tokio::test]
    async fn cancel_delivery_failure_tears_down_the_inflight_process_scope() {
        let runtime = AcpRuntime::new();
        let limits = RuntimeLimits::new(60, 1);
        let agent = ConfiguredAgent {
            id: "closed-cancel-transport".into(),
            name: "Closed cancel transport".into(),
            enabled: true,
            source: "custom".into(),
            command: "sh".into(),
            args: vec!["-c".into(), "sleep 30".into()],
            env: HashMap::new(),
            icon: None,
            sort: 0,
        };
        let anchor = spawn_process_anchor(&agent, false, limits, runtime.permissions.clone())
            .expect("spawn process anchor");
        let live = spawn_logical_session(
            &anchor,
            &agent,
            std::env::current_dir().expect("current directory"),
            false,
            limits,
            runtime.permissions.clone(),
        );
        live.prompt_generation.store(1, Ordering::Release);
        live.prompt_state.store(PROMPT_RUNNING, Ordering::Release);
        live.active.lock().await.id = Some(SessionId::new("session-1"));
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        *live.event_slot.lock().await = Some(event_tx);
        runtime
            .warm_sessions
            .lock()
            .await
            .insert(anchor.fingerprint.clone(), anchor);
        runtime
            .sessions
            .lock()
            .await
            .insert("thread-a".into(), live.clone());

        assert!(
            tokio::time::timeout(Duration::from_secs(2), runtime.cancel("thread-a"))
                .await
                .expect("cancel failure teardown is bounded")
                .expect("cancel reports handled")
        );
        assert!(live.process_shutdown.load(Ordering::Acquire));
        assert!(!runtime.has_live_session("thread-a").await);
        assert!(matches!(
            event_rx.try_recv(),
            Ok(AcpEvent::Status { message })
                if message == ACP_STATUS_CANCEL_RESTARTING
        ));
    }

    #[test]
    fn parses_grok_retry_extension_status() {
        let params = serde_json::value::to_raw_value(&serde_json::json!({
            "session_id": "session-42",
            "update": {
                "session_update": "retry_state",
                "attempt": 3,
                "maxRetries": 15,
                "status": "rate limited"
            }
        }))
        .map(Arc::from)
        .expect("encode extension params");
        let notification = ExtNotification::new("_x.ai/session_notification", params);

        let (session_id, message) = grok_retry_status(&notification).expect("retry status");

        assert_eq!(session_id.to_string(), "session-42");
        assert_eq!(
            message,
            r#"aqbot:grok-retry:{"attempt":3,"maximum":15,"detail":"rate limited"}"#
        );
    }

    fn prompt_attachment(mime_type: &str, data: Option<&str>) -> AcpPromptAttachment {
        AcpPromptAttachment {
            file_name: if is_image_mime_type(mime_type) {
                "diagram.png".into()
            } else {
                "notes.md".into()
            },
            mime_type: mime_type.into(),
            file_size: 42,
            data: data.map(str::to_owned),
            file_uri: if is_image_mime_type(mime_type) {
                "file:///tmp/diagram.png".into()
            } else {
                "file:///tmp/notes.md".into()
            },
        }
    }

    #[test]
    fn builds_text_image_and_resource_link_prompt_blocks() {
        let input = AcpPromptInput {
            text: "Explain these files".into(),
            attachments: vec![
                prompt_attachment("image/png", Some("aW1hZ2U=")),
                prompt_attachment("text/markdown", None),
            ],
        };
        let capabilities =
            AgentCapabilities::new().prompt_capabilities(PromptCapabilities::new().image(true));

        let blocks = prompt_content_blocks(&input, &capabilities).expect("valid prompt blocks");

        assert_eq!(blocks.len(), 3);
        assert!(matches!(
            &blocks[0],
            ContentBlock::Text(content) if content.text == "Explain these files"
        ));
        assert!(matches!(
            &blocks[1],
            ContentBlock::Image(content)
                if content.data == "aW1hZ2U="
                    && content.mime_type == "image/png"
                    && content.uri.as_deref() == Some("file:///tmp/diagram.png")
        ));
        assert!(matches!(
            &blocks[2],
            ContentBlock::ResourceLink(resource)
                if resource.name == "notes.md"
                    && resource.mime_type.as_deref() == Some("text/markdown")
                    && resource.size == Some(42)
                    && resource.uri == "file:///tmp/notes.md"
        ));
    }

    #[test]
    fn resource_links_do_not_require_optional_prompt_capabilities() {
        let input = AcpPromptInput {
            text: String::new(),
            attachments: vec![prompt_attachment("application/pdf", None)],
        };

        let blocks = prompt_content_blocks(&input, &AgentCapabilities::default())
            .expect("resource links are an ACP baseline capability");

        assert!(matches!(blocks.as_slice(), [ContentBlock::ResourceLink(_)]));
    }

    #[test]
    fn rejects_images_without_the_advertised_capability_or_payload() {
        let input = AcpPromptInput {
            text: String::new(),
            attachments: vec![prompt_attachment("image/png", Some("aW1hZ2U="))],
        };
        let error = prompt_content_blocks(&input, &AgentCapabilities::default())
            .expect_err("image capability is mandatory");
        assert!(error.to_string().contains("image prompt capability"));

        let uppercase_input = AcpPromptInput {
            text: String::new(),
            attachments: vec![prompt_attachment("IMAGE/PNG", Some("aW1hZ2U="))],
        };
        let error = prompt_content_blocks(&uppercase_input, &AgentCapabilities::default())
            .expect_err("MIME matching must not bypass image capability");
        assert!(error.to_string().contains("image prompt capability"));

        let disguised_image = AcpPromptInput {
            text: String::new(),
            attachments: vec![AcpPromptAttachment {
                file_name: "diagram.PNG".into(),
                mime_type: "application/x-custom".into(),
                file_size: 42,
                data: Some("aW1hZ2U=".into()),
                file_uri: "file:///tmp/diagram.PNG".into(),
            }],
        };
        let error = prompt_content_blocks(&disguised_image, &AgentCapabilities::default())
            .expect_err("image extensions must not bypass image capability");
        assert!(error.to_string().contains("image prompt capability"));
        let capabilities =
            AgentCapabilities::new().prompt_capabilities(PromptCapabilities::new().image(true));
        let blocks = prompt_content_blocks(&disguised_image, &capabilities)
            .expect("supported image extension is normalized");
        assert!(matches!(
            blocks.as_slice(),
            [ContentBlock::Image(image)] if image.mime_type == "image/png"
        ));

        let input = AcpPromptInput {
            text: String::new(),
            attachments: vec![prompt_attachment("image/png", None)],
        };
        let capabilities =
            AgentCapabilities::new().prompt_capabilities(PromptCapabilities::new().image(true));
        let error =
            prompt_content_blocks(&input, &capabilities).expect_err("image data is mandatory");
        assert!(error.to_string().contains("no Base64 payload"));
    }

    #[test]
    fn rejects_an_empty_prompt_input() {
        let error = prompt_content_blocks(
            &AcpPromptInput {
                text: String::new(),
                attachments: Vec::new(),
            },
            &AgentCapabilities::default(),
        )
        .expect_err("prompt must contain a block");
        assert!(error.to_string().contains("text or an attachment"));
    }

    #[tokio::test]
    async fn prompt_handle_surfaces_a_worker_exit() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let permissions = Arc::new(Mutex::new(HashMap::new()));
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let (pending, _selected_rx) = pending_permission("scope-1", event_tx);
        permissions.lock().await.insert("request-1".into(), pending);
        let (reply_tx, reply_rx) = oneshot::channel();
        drop(reply_tx);
        let handle = AcpPromptHandle {
            session_key: "thread-1".into(),
            permission_scope: "scope-1".into(),
            permissions,
            sessions,
            reply_rx,
        };

        let error = handle.wait().await.expect_err("closed worker must fail");

        assert!(error.to_string().contains("session worker exited"));
        assert!(matches!(
            event_rx.recv().await,
            Some(AcpEvent::InteractionClosed {
                request_id,
                outcome: AcpInteractionOutcome::Cancelled,
                ..
            }) if request_id == "request-1"
        ));
    }

    #[test]
    fn extended_new_session_request_keeps_required_standard_fields() {
        let request = ExtendedNewSessionRequest::new(PathBuf::from("/tmp/project"));
        let serialized = serde_json::to_value(request).expect("serialize session/new request");
        assert_eq!(
            serialized.get("cwd").and_then(|value| value.as_str()),
            Some("/tmp/project")
        );
        assert_eq!(serialized.get("mcpServers"), Some(&serde_json::json!([])));
    }

    #[test]
    fn extended_new_session_response_skips_future_config_kinds_and_keeps_extensions() {
        let response: ExtendedNewSessionResponse = serde_json::from_value(serde_json::json!({
            "sessionId": "session-forward-compatible",
            "configOptions": [
                {
                    "id": "mode",
                    "name": "Mode",
                    "category": "mode",
                    "type": "select",
                    "currentValue": "default",
                    "options": [{ "value": "default", "name": "Default" }]
                },
                {
                    "id": "future-control",
                    "name": "Future control",
                    "type": "not-yet-supported-by-this-client",
                    "currentValue": { "level": 2 }
                }
            ],
            "models": {
                "currentModelId": "model-a",
                "availableModels": [{ "modelId": "model-a" }]
            },
            "reasoningEfforts": [{ "id": "high", "label": "High" }],
            "_meta": { "vendor/session": true }
        }))
        .expect("a future config kind must not reject session/new");

        let config_options = response
            .standard
            .config_options
            .as_ref()
            .expect("valid config option remains available");
        assert_eq!(config_options.len(), 1);
        assert_eq!(config_options[0].id.to_string(), "mode");
        assert_eq!(
            response
                .models
                .as_ref()
                .and_then(|models| models.get("currentModelId"))
                .and_then(serde_json::Value::as_str),
            Some("model-a")
        );
        assert_eq!(
            response.reasoning_efforts,
            Some(serde_json::json!([{ "id": "high", "label": "High" }]))
        );
        assert_eq!(
            response
                .standard
                .meta
                .as_ref()
                .and_then(|meta| meta.get("vendor/session")),
            Some(&serde_json::Value::Bool(true))
        );
    }

    #[test]
    fn user_message_echo_is_never_rendered_as_assistant_output() {
        assert!(is_assistant_message_update("agent_message_chunk"));
        assert!(!is_assistant_message_update("user_message_chunk"));
    }

    #[test]
    fn retries_only_explicit_missing_session_errors() {
        assert!(is_missing_session_error("Session not found"));
        assert!(is_missing_session_error("code=session_not_found"));
        assert!(is_missing_session_error(
            "Resource not found: Session 01abc not found: {uri: Session 01abc not found}"
        ));
        assert!(!is_missing_session_error(
            "session/prompt failed: session rate limit exceeded"
        ));
        assert!(!is_missing_session_error("session database unavailable"));
        assert!(!is_missing_session_error("connection closed"));
    }

    #[test]
    fn maps_legacy_model_state_to_a_live_selector() {
        let state = serde_json::json!({
            "currentModelId": "grok-4.5",
            "availableModels": [
                { "modelId": "grok-4.5", "name": "Grok 4.5" },
                { "modelId": "grok-code", "name": "Grok Code" }
            ]
        });
        let option = legacy_model_option_from_state(&state).expect("model selector");
        assert_eq!(option.category, Some(SessionConfigOptionCategory::Model));
        assert_eq!(option.id.to_string(), "model");
        assert_eq!(
            option
                .meta
                .as_ref()
                .and_then(|meta| meta.get("aqbotSetMethod")),
            Some(&serde_json::Value::String("session/set_model".into()))
        );
        let SessionConfigKind::Select(select) = option.kind else {
            panic!("expected select option");
        };
        assert_eq!(select.current_value.to_string(), "grok-4.5");
        let SessionConfigSelectOptions::Ungrouped(choices) = select.options else {
            panic!("expected flat model choices");
        };
        assert_eq!(choices.len(), 2);
    }

    #[test]
    fn maps_grok_model_metadata_to_live_reasoning_selector() {
        let state = serde_json::json!({
            "currentModelId": "grok-4.5",
            "availableModels": [{
                "modelId": "grok-4.5",
                "name": "Grok 4.5",
                "_meta": {
                    "reasoningEffort": "high",
                    "reasoningEfforts": [
                        { "id": "high", "value": "high", "label": "High Effort", "default": true },
                        { "id": "medium", "value": "medium", "label": "Medium Effort", "default": false }
                    ]
                }
            }]
        });
        let option = legacy_reasoning_option_from_state(&state).expect("reasoning selector");
        assert_eq!(
            option.category,
            Some(SessionConfigOptionCategory::ThoughtLevel)
        );
        assert_eq!(option.id.to_string(), "reasoning_effort");
        assert_eq!(
            option
                .meta
                .as_ref()
                .and_then(|meta| meta.get("aqbotSetMethod")),
            Some(&serde_json::Value::String(
                "session/set_model_reasoning".into()
            ))
        );
        let SessionConfigKind::Select(select) = option.kind else {
            panic!("expected select option");
        };
        assert_eq!(select.current_value.to_string(), "high");
    }

    #[test]
    fn grok_reasoning_update_uses_set_model_metadata_without_restarting() {
        let request = LegacySetModelRequest::with_reasoning(
            SessionId::new("session-1"),
            "grok-4.5",
            "medium",
        );
        assert_eq!(
            serde_json::to_value(request).expect("serialize reasoning update"),
            serde_json::json!({
                "sessionId": "session-1",
                "modelId": "grok-4.5",
                "_meta": { "reasoningEffort": "medium" }
            })
        );
    }

    #[test]
    fn places_grok_reasoning_flag_before_stdio_and_replaces_old_value() {
        let agent = ConfiguredAgent {
            id: "grok-build".into(),
            name: "Grok Build".into(),
            enabled: true,
            source: "registry".into(),
            command: "grok".into(),
            args: vec![
                "agent".into(),
                "--reasoning-effort".into(),
                "low".into(),
                "stdio".into(),
            ],
            env: HashMap::new(),
            icon: None,
            sort: 0,
        };
        let updated =
            configured_agent_with_reasoning_effort(&agent, "medium").expect("valid spawn args");
        assert_eq!(
            updated.args,
            ["agent", "--reasoning-effort", "medium", "stdio"]
        );
        assert!(configured_agent_with_reasoning_effort(&agent, "bad value").is_err());
    }

    #[test]
    fn places_copilot_model_before_transport_and_restores_default() {
        let agent = ConfiguredAgent {
            id: "github-copilot-cli".into(),
            name: "GitHub Copilot".into(),
            enabled: true,
            source: "registry".into(),
            command: "npx".into(),
            args: vec![
                "-y".into(),
                "@github/copilot@1.0.78".into(),
                "--model=auto".into(),
                "--acp".into(),
            ],
            env: HashMap::new(),
            icon: None,
            sort: 0,
        };
        let selected = configured_agent_with_model(&agent, "gpt-5.6-sol").expect("model args");
        assert_eq!(
            selected.args,
            [
                "-y",
                "@github/copilot@1.0.78",
                "--model",
                "gpt-5.6-sol",
                "--acp"
            ]
        );
        let restored = configured_agent_with_model(&selected, "__agent_default")
            .expect("remove model override");
        assert_eq!(restored.args, ["-y", "@github/copilot@1.0.78", "--acp"]);
    }

    #[test]
    fn parses_copilot_cli_model_and_reasoning_catalogs() {
        let config_help = r#"
          `model`: AI model to use.
            - "claude-sonnet-4.6"
            - "gpt-5.6-sol"

          `contextTier`: context window tier.
        "#;
        let command_help = r#"
          --effort, --reasoning-effort <level> Set effort (choices: "none",
                                               "low", "medium", "high", "max")
        "#;
        assert_eq!(
            parse_copilot_models(config_help),
            ["claude-sonnet-4.6", "gpt-5.6-sol"]
        );
        assert_eq!(
            parse_copilot_reasoning_efforts(command_help),
            ["none", "low", "medium", "high", "max"]
        );
    }

    #[test]
    fn discovered_copilot_models_use_the_live_structured_setter() {
        let option = launch_live_model_option(
            "__agent_default".into(),
            &["auto".into(), "gpt-5.6-sol".into()],
        );
        let meta = option.meta.as_ref().expect("host route metadata");
        assert_eq!(
            meta.get("aqbotSetMethod"),
            Some(&serde_json::Value::String("session/set_model".into()))
        );
        assert!(!meta.contains_key("aqbotSpawnArg"));
        let SessionConfigKind::Select(select) = &option.kind else {
            panic!("expected model selector");
        };
        assert_eq!(select.current_value.to_string(), "auto");
        let SessionConfigSelectOptions::Ungrouped(choices) = &select.options else {
            panic!("expected flat model choices");
        };
        assert!(choices
            .iter()
            .all(|choice| choice.value.to_string() != "__agent_default"));
    }

    #[test]
    fn grok_exit_plan_mode_uses_the_verified_wire_contract() {
        let request: GrokExitPlanModeRequest = serde_json::from_value(serde_json::json!({
            "sessionId": "session-1",
            "toolCallId": "call-plan-1",
            "planContent": "## Plan\n1. Inspect\n2. Test"
        }))
        .expect("parse Grok plan review");
        assert_eq!(request.session_id.to_string(), "session-1");
        assert_eq!(request.tool_call_id.as_deref(), Some("call-plan-1"));
        assert_eq!(
            serde_json::to_value(GrokExitPlanModeResponse::new("approved"))
                .expect("serialize plan response"),
            serde_json::json!({ "outcome": "approved" })
        );
    }

    #[test]
    fn grok_questionnaire_preserves_question_option_and_freeform_contract() {
        let request: GrokAskUserRequest = serde_json::from_value(serde_json::json!({
            "sessionId": "session-1",
            "toolCallId": "call-ask-1",
            "mode": "plan",
            "questions": [
                {
                    "id": "layers",
                    "question": "Which layers?",
                    "multiSelect": true,
                    "options": [
                        { "id": "ui", "label": "Frontend", "description": "Web UI" },
                        { "id": "api", "label": "Backend", "description": "Rust API" }
                    ]
                },
                {
                    "question": "Which store?",
                    "multiSelect": false,
                    "options": [{
                        "id": "postgres-id",
                        "label": "Postgres",
                        "preview": "CREATE TABLE events (...);"
                    }]
                },
                {
                    "question": "Anything else?",
                    "options": []
                }
            ]
        }))
        .expect("parse Grok question");
        assert_eq!(request.mode, GrokAskUserMode::Plan);
        assert!(request.questions[0].multi_select);
        assert_eq!(request.questions[0].id.as_deref(), Some("layers"));
        assert_eq!(
            request.questions[1].options[0].id.as_deref(),
            Some("postgres-id")
        );

        // Deliberately submit questions and choices out of order. The host must
        // map indexes back to the original Agent-provided order.
        let submission = AcpQuestionnaireSubmission {
            outcome: AcpQuestionnaireOutcome::Accepted,
            answers: vec![
                AcpQuestionnaireAnswer {
                    question_index: 2,
                    selected_option_indexes: vec![],
                    other_text: Some("  请使用中文  ".into()),
                },
                AcpQuestionnaireAnswer {
                    question_index: 1,
                    selected_option_indexes: vec![0],
                    other_text: None,
                },
                AcpQuestionnaireAnswer {
                    question_index: 0,
                    selected_option_indexes: vec![1, 0],
                    other_text: Some("  Keep mobile unchanged  ".into()),
                },
            ],
        };
        let context = GrokQuestionnaireContext {
            questions: request.questions.clone(),
            mode: request.mode,
        };
        validate_questionnaire_submission(&context, &submission)
            .expect("valid multi-question submission");
        let response = GrokAskUserResponse::from_submission(&request, &submission);
        let GrokAskUserResponse::Accepted {
            answers,
            annotations,
        } = &response
        else {
            panic!("expected accepted response");
        };
        assert_eq!(
            answers.keys().map(String::as_str).collect::<Vec<_>>(),
            vec!["Which layers?", "Which store?", "Anything else?"]
        );
        assert_eq!(answers["Which layers?"], ["Frontend", "Backend"]);
        assert_eq!(answers["Which store?"], ["Postgres"]);
        assert_eq!(answers["Anything else?"], ["Other"]);
        let annotations = annotations.as_ref().expect("answer annotations");
        assert_eq!(
            annotations["Which layers?"].notes.as_deref(),
            Some("  Keep mobile unchanged  ")
        );
        assert_eq!(
            annotations["Which store?"].preview.as_deref(),
            Some("CREATE TABLE events (...);")
        );
        assert_eq!(
            annotations["Anything else?"].notes.as_deref(),
            Some("  请使用中文  ")
        );

        let serialized = serde_json::to_string(&response).expect("serialize accepted answer");
        assert!(!serialized.contains("postgres-id"));
        assert!(serialized.find("Which layers?") < serialized.find("Which store?"));
        assert!(serialized.find("Which store?") < serialized.find("Anything else?"));
        assert_eq!(
            serde_json::to_value(response).expect("serialize accepted answer"),
            serde_json::json!({
                "outcome": "accepted",
                "answers": {
                    "Which layers?": ["Frontend", "Backend"],
                    "Which store?": ["Postgres"],
                    "Anything else?": ["Other"]
                },
                "annotations": {
                    "Which layers?": { "notes": "  Keep mobile unchanged  " },
                    "Which store?": { "preview": "CREATE TABLE events (...);" },
                    "Anything else?": { "notes": "  请使用中文  " }
                }
            })
        );
    }

    #[test]
    fn grok_questionnaire_serializes_plan_and_cancel_outcomes_exactly() {
        let request: GrokAskUserRequest = serde_json::from_value(serde_json::json!({
            "sessionId": "session-1",
            "mode": "plan",
            "questions": [
                {
                    "question": "Which layers?",
                    "multiSelect": true,
                    "options": [
                        { "label": "Frontend" },
                        { "label": "Backend" }
                    ]
                },
                { "question": "Anything else?", "options": [] }
            ]
        }))
        .expect("parse Grok plan questionnaire");
        let answers = vec![
            AcpQuestionnaireAnswer {
                question_index: 1,
                selected_option_indexes: vec![],
                other_text: Some("notes are intentionally omitted on this wire shape".into()),
            },
            AcpQuestionnaireAnswer {
                question_index: 0,
                selected_option_indexes: vec![1, 0],
                other_text: None,
            },
        ];

        for (outcome, expected_outcome) in [
            (AcpQuestionnaireOutcome::ChatAboutThis, "chat_about_this"),
            (AcpQuestionnaireOutcome::SkipInterview, "skip_interview"),
        ] {
            let submission = AcpQuestionnaireSubmission {
                outcome,
                answers: answers.clone(),
            };
            let response = GrokAskUserResponse::from_submission(&request, &submission);
            assert_eq!(
                serde_json::to_value(response).expect("serialize plan questionnaire response"),
                serde_json::json!({
                    "outcome": expected_outcome,
                    "partial_answers": {
                        "Which layers?": "Frontend, Backend",
                        "Anything else?": "Other"
                    }
                })
            );
        }

        assert_eq!(
            serde_json::to_value(GrokAskUserResponse::cancelled())
                .expect("serialize cancelled answer"),
            serde_json::json!({ "outcome": "cancelled" })
        );
    }

    #[test]
    fn grok_questionnaire_rejects_invalid_or_plan_only_submissions() {
        let question = GrokQuestion {
            question: "Choose one".into(),
            multi_select: false,
            options: vec![GrokQuestionOption {
                label: "A".into(),
                description: None,
                preview: None,
                id: None,
            }],
            id: None,
        };
        let context = GrokQuestionnaireContext {
            questions: vec![question],
            mode: GrokAskUserMode::Default,
        };
        let plan_action = AcpQuestionnaireSubmission {
            outcome: AcpQuestionnaireOutcome::ChatAboutThis,
            answers: vec![],
        };
        assert!(validate_questionnaire_submission(&context, &plan_action)
            .expect_err("default mode must reject plan action")
            .contains("outside plan mode"));

        let ambiguous_single_choice = AcpQuestionnaireSubmission {
            outcome: AcpQuestionnaireOutcome::Accepted,
            answers: vec![AcpQuestionnaireAnswer {
                question_index: 0,
                selected_option_indexes: vec![0],
                other_text: Some("Other choice".into()),
            }],
        };
        assert!(
            validate_questionnaire_submission(&context, &ambiguous_single_choice)
                .expect_err("single choice cannot include an option and Other")
                .contains("only accepts one answer")
        );
    }

    #[test]
    fn grok_session_selection_overrides_catalog_default_effort() {
        let state = serde_json::json!({
            "currentModelId": "grok-4.5",
            "availableModels": [{
                "modelId": "grok-4.5",
                "_meta": {
                    "reasoningEffort": "high",
                    "reasoningEfforts": [
                        { "value": "high", "label": "High" },
                        { "value": "medium", "label": "Medium" }
                    ]
                }
            }]
        });
        let mut options =
            vec![legacy_reasoning_option_from_state(&state).expect("reasoning selector")];
        let mut meta = serde_json::Map::new();
        meta.insert(
            "x.ai/sessionConfig".into(),
            serde_json::json!({
                "options": [
                    { "id": "high", "category": "mode", "selected": false },
                    { "id": "medium", "category": "mode", "selected": true }
                ]
            }),
        );
        apply_legacy_session_selection(&mut options, Some(&meta));
        let SessionConfigKind::Select(select) = &options[0].kind else {
            panic!("expected select option");
        };
        assert_eq!(select.current_value.to_string(), "medium");
    }

    #[test]
    fn standard_model_config_takes_precedence_over_legacy_metadata() {
        let standard = SessionConfigOption::select(
            "model",
            "Model",
            "standard",
            vec![SessionConfigSelectOption::new("standard", "Standard")],
        )
        .category(SessionConfigOptionCategory::Model);
        let mut meta = serde_json::Map::new();
        meta.insert(
            "modelState".into(),
            serde_json::json!({
                "currentModelId": "legacy",
                "availableModels": [{ "modelId": "legacy" }]
            }),
        );
        let metadata = AgentMetadata {
            capabilities: AgentCapabilities::default(),
            meta: Some(meta),
            launch_config_options: Vec::new(),
        };
        let options = normalized_config_options(vec![standard], &metadata);
        assert_eq!(options.len(), 1);
    }

    #[test]
    fn launch_refresh_preserves_native_same_id_config() {
        let mut native_meta = agent_client_protocol::schema::v1::Meta::new();
        native_meta.insert("vendorNative".into(), serde_json::Value::Bool(true));
        let native = SessionConfigOption::select(
            "model",
            "Native model",
            "native-b",
            vec![
                SessionConfigSelectOption::new("native-a", "Native A"),
                SessionConfigSelectOption::new("native-b", "Native B"),
            ],
        )
        .category(SessionConfigOptionCategory::Model)
        .meta(native_meta);
        let fallback = SessionConfigOption::select(
            "model",
            "CLI fallback",
            "fallback-a",
            vec![SessionConfigSelectOption::new("fallback-a", "Fallback A")],
        )
        .category(SessionConfigOptionCategory::Model);
        let metadata = AgentMetadata {
            capabilities: AgentCapabilities::default(),
            meta: None,
            launch_config_options: vec![fallback],
        };

        let retained = agent_options_for_launch_refresh(std::slice::from_ref(&native));
        let refreshed = normalized_config_options_for_session(retained, &metadata, &[native]);
        let value = serde_json::to_value(&refreshed).expect("serialize refreshed config");

        assert_eq!(refreshed.len(), 1);
        assert_eq!(value[0]["name"], "Native model");
        assert_eq!(value[0]["currentValue"], "native-b");
        assert_eq!(value[0]["_meta"]["vendorNative"], true);
        assert_eq!(value[0]["options"].as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn agent_permission_config_overrides_global_auto_approval() {
        let permission = SessionConfigOption::select(
            "mode",
            "Permission",
            "read-only",
            vec![SessionConfigSelectOption::new(
                "read-only",
                "Request approval",
            )],
        )
        .category(SessionConfigOptionCategory::Mode);
        let collaboration = SessionConfigOption::select(
            "collaboration_mode",
            "Collaboration",
            "plan",
            vec![SessionConfigSelectOption::new("plan", "Plan")],
        )
        .category(SessionConfigOptionCategory::Mode);
        assert!(has_agent_permission_config(&[permission]));
        assert!(!has_agent_permission_config(&[collaboration]));
    }

    #[test]
    fn claude_mixed_permission_and_plan_mode_overrides_global_auto_approval() {
        let claude_mode = SessionConfigOption::select(
            "mode",
            "Mode",
            "default",
            vec![
                SessionConfigSelectOption::new("default", "Manual"),
                SessionConfigSelectOption::new("acceptEdits", "Accept Edits"),
                SessionConfigSelectOption::new("plan", "Plan Mode"),
                SessionConfigSelectOption::new("bypassPermissions", "Bypass Permissions"),
            ],
        )
        .description("Session permission mode")
        .category(SessionConfigOptionCategory::Mode);

        assert!(has_agent_permission_config(&[claude_mode]));
    }

    #[test]
    fn synthesizes_only_verified_grok_permission_and_plan_controls() {
        let mut meta = serde_json::Map::new();
        meta.insert("grokShell".into(), serde_json::Value::Bool(true));
        let metadata = AgentMetadata {
            capabilities: AgentCapabilities::default(),
            meta: Some(meta),
            launch_config_options: Vec::new(),
        };

        let modes = normalized_session_modes(None, &metadata).expect("Grok session modes");
        assert_eq!(modes.current_mode_id.to_string(), "default");
        assert_eq!(
            modes
                .available_modes
                .iter()
                .map(|mode| mode.id.to_string())
                .collect::<Vec<_>>(),
            ["default", "plan"]
        );

        let options = normalized_config_options(Vec::new(), &metadata);
        let permission = options
            .iter()
            .find(|option| option.id.to_string() == GROK_PERMISSION_CONFIG_ID)
            .expect("Grok permission control");
        assert_eq!(
            permission.category,
            Some(SessionConfigOptionCategory::Other("permissions".into()))
        );
        let SessionConfigKind::Select(select) = &permission.kind else {
            panic!("expected select permission control");
        };
        let SessionConfigSelectOptions::Ungrouped(choices) = &select.options else {
            panic!("expected flat permission choices");
        };
        assert_eq!(
            choices
                .iter()
                .map(|choice| choice.value.to_string())
                .collect::<Vec<_>>(),
            ["default", "auto", "bypassPermissions"]
        );
    }

    #[test]
    fn detects_permission_modes_without_misclassifying_behavior_or_uri_plan_modes() {
        let gemini = SessionModeState::new(
            "default",
            vec![
                SessionMode::new("default", "Default"),
                SessionMode::new("auto_edit", "Auto Edit"),
                SessionMode::new("yolo", "YOLO"),
                SessionMode::new("plan", "Plan"),
            ],
        );
        let behavior = SessionModeState::new(
            "concise",
            vec![
                SessionMode::new("concise", "Concise"),
                SessionMode::new("verbose", "Verbose"),
                SessionMode::new("plan", "Plan"),
            ],
        );
        let copilot = SessionModeState::new(
            "https://agentclientprotocol.com/protocol/session-modes#agent",
            vec![
                SessionMode::new(
                    "https://agentclientprotocol.com/protocol/session-modes#agent",
                    "Agent",
                ),
                SessionMode::new(
                    "https://agentclientprotocol.com/protocol/session-modes#plan",
                    "Plan",
                ),
                SessionMode::new(
                    "https://agentclientprotocol.com/protocol/session-modes#autopilot",
                    "Autopilot",
                ),
            ],
        );

        assert!(has_agent_permission_modes(Some(&gemini)));
        assert!(!has_agent_permission_modes(Some(&behavior)));
        assert!(!has_agent_permission_modes(Some(&copilot)));
    }

    #[test]
    fn native_session_modes_take_precedence_over_grok_adapter_modes() {
        let mut meta = agent_client_protocol::schema::v1::Meta::new();
        meta.insert("grokShell".into(), serde_json::Value::Bool(true));
        let metadata = AgentMetadata {
            capabilities: AgentCapabilities::default(),
            meta: Some(meta),
            launch_config_options: Vec::new(),
        };
        let native = SessionModeState::new("native", vec![SessionMode::new("native", "Native")]);

        assert_eq!(
            normalized_session_modes(Some(native), &metadata)
                .expect("native modes")
                .current_mode_id
                .to_string(),
            "native"
        );
    }

    #[test]
    fn strips_agent_supplied_host_routing_metadata() {
        let mut marker = agent_client_protocol::schema::v1::Meta::new();
        marker.insert(
            "aqbotSpawnArg".into(),
            serde_json::Value::String("--unsafe-agent-controlled-flag".into()),
        );
        marker.insert("vendorHint".into(), serde_json::Value::Bool(true));
        let option = SessionConfigOption::select(
            "vendor-control",
            "Vendor Control",
            "off",
            vec![SessionConfigSelectOption::new("off", "Off")],
        )
        .meta(marker);
        let metadata = AgentMetadata {
            capabilities: AgentCapabilities::default(),
            meta: None,
            launch_config_options: Vec::new(),
        };

        let normalized = normalized_config_options(vec![option], &metadata);
        let meta = normalized[0]
            .meta
            .as_ref()
            .expect("vendor metadata remains");
        assert!(!meta.contains_key("aqbotSpawnArg"));
        assert_eq!(meta.get("vendorHint"), Some(&serde_json::Value::Bool(true)));
    }

    #[test]
    fn separates_copilot_uri_plan_mode_from_permission_config() {
        let plan = SessionConfigOption::select(
            "mode",
            "Mode",
            "https://agentclientprotocol.com/protocol/session-modes#agent",
            vec![
                SessionConfigSelectOption::new(
                    "https://agentclientprotocol.com/protocol/session-modes#agent",
                    "Agent",
                ),
                SessionConfigSelectOption::new(
                    "https://agentclientprotocol.com/protocol/session-modes#plan",
                    "Plan",
                ),
            ],
        )
        .category(SessionConfigOptionCategory::Mode);
        let permission = SessionConfigOption::select(
            "allow_all",
            "Allow All",
            "off",
            vec![
                SessionConfigSelectOption::new("on", "On"),
                SessionConfigSelectOption::new("off", "Off"),
            ],
        )
        .category(SessionConfigOptionCategory::Other("permissions".into()));

        assert!(config_option_contains_plan(&plan));
        assert!(!has_agent_permission_config(&[plan]));
        assert!(has_agent_permission_config(&[permission]));
    }

    #[test]
    fn persists_config_backed_plan_with_its_config_id() {
        let collaboration = SessionConfigOption::select(
            "collaboration_mode",
            "Collaboration",
            "plan",
            vec![
                SessionConfigSelectOption::new("default", "Default"),
                SessionConfigSelectOption::new("plan", "Plan"),
            ],
        )
        .category(SessionConfigOptionCategory::Mode);
        let snapshot = AcpSessionSnapshot {
            session_id: "session-1".into(),
            modes: None,
            config_options: vec![collaboration],
            agent_capabilities: AgentCapabilities::default(),
        };

        let persisted = persisted_mode_id(&snapshot).expect("config plan is persisted");
        assert!(persisted.starts_with(PERSISTED_CONFIG_MODE_PREFIX));
        assert!(persisted.contains("collaboration_mode"));
        assert!(persisted.contains("plan"));
    }
}
