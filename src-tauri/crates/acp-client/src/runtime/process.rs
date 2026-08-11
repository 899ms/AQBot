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

#[derive(Debug, thiserror::Error)]
#[error("{method} timed out after {timeout_ms} ms")]
struct SessionControlTimeout {
    method: &'static str,
    timeout_ms: u128,
}

fn is_session_control_timeout(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.downcast_ref::<SessionControlTimeout>().is_some())
}

async fn session_control_request<T, E>(
    method: &'static str,
    timeout: Duration,
    request: impl Future<Output = Result<T, E>>,
) -> anyhow::Result<T>
where
    E: std::fmt::Display,
{
    match tokio::time::timeout(timeout, request).await {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(error)) => Err(anyhow::anyhow!("{method} failed: {error}")),
        Err(_) => Err(SessionControlTimeout {
            method,
            timeout_ms: timeout.as_millis(),
        }
        .into()),
    }
}

fn live_session_control_timeout(live: &LiveSession) -> anyhow::Result<Duration> {
    Ok(live
        .runtime_limits
        .lock()
        .map_err(|_| anyhow::anyhow!("ACP runtime limits lock is poisoned"))?
        .session_control_timeout)
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
                    flush_routed_session_notifications(
                        &notification_routes,
                        &notification_metadata,
                    )
                    .await;
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
        let permissions_elicitation = permissions.clone();
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
                    move |request: ExtendedRequestPermissionRequest,
                          responder: Responder<ExtendedRequestPermissionResponse>,
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
                                        return responder
                                            .respond(ExtendedRequestPermissionResponse::cancelled());
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
                                    responder.respond(ExtendedRequestPermissionResponse::cancelled())
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
                    let permissions = permissions_elicitation;
                    let routes = routes.clone();
                    move |request: CreateElicitationRequest,
                          responder: Responder<CreateElicitationResponse>,
                          connection: ConnectionTo<Agent>| {
                        let permissions = permissions.clone();
                        let routes = routes.clone();
                        async move {
                            connection.spawn(async move {
                                let session_id = match request.scope() {
                                    ElicitationScope::Session(scope) => scope.session_id.clone(),
                                    _ => {
                                        return responder.respond(CreateElicitationResponse::new(
                                            ElicitationAction::Cancel,
                                        ));
                                    }
                                };
                                let route = resolve_session_route(&routes, &session_id).await;
                                if let Some(route) = route {
                                    if route.prompt_state.load(Ordering::Acquire)
                                        == PROMPT_CANCEL_REQUESTED
                                    {
                                        return responder.respond(CreateElicitationResponse::new(
                                            ElicitationAction::Cancel,
                                        ));
                                    }
                                    let event_tx = route.event_slot.lock().await.clone();
                                    handle_elicitation_request(
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
                                    responder.respond(CreateElicitationResponse::new(
                                        ElicitationAction::Cancel,
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
                        let initialize = aqbot_initialize_request();
                        match connection.send_request(initialize).block_task().await {
                            Ok(response) => {
                                if response.protocol_version != ProtocolVersion::V1 {
                                    let msg = format!(
                                        "initialize failed: unsupported ACP protocol version {}; only version 1 is supported",
                                        response.protocol_version
                                    );
                                    let _ = ready_tx.send(ReadyState::Failed(msg.clone()));
                                    return Err(agent_client_protocol::util::internal_error(msg));
                                }
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
    let worker_session_control_timeout = limits.session_control_timeout;
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
                                        worker_session_control_timeout,
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
