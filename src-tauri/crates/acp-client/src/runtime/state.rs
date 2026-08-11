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
    pending_notifications: HashMap<String, Vec<SessionNotification>>,
    routed_notifications: Vec<(SessionRoute, SessionNotification)>,
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
