// ACP session preparation, prewarming, and cancellation.

// ---------- Prompt / permission ----------

fn runtime_limits(config: &AcpAgentsFile) -> RuntimeLimits {
    RuntimeLimits::new(
        config.general.idle_timeout_secs,
        config.general.max_concurrent_processes,
    )
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpPrewarmResult {
    agent_id: String,
    ready: bool,
    started: bool,
    error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpPromptAccepted {
    user_message: acp_repo::AcpMessageView,
    assistant_message: acp_repo::AcpMessageView,
}

struct PreparedPrewarm {
    agents: Vec<ConfiguredAgent>,
    auto_approve: bool,
    limits: RuntimeLimits,
    launch_generation: u64,
}

struct LockedPreparedPrewarm {
    prepared: PreparedPrewarm,
    guard: tokio::sync::MutexGuard<'static, ()>,
}

#[tauri::command]
pub async fn acp_prewarm_enabled_agents(
    state: State<'_, AppState>,
) -> Result<Vec<AcpPrewarmResult>, String> {
    let first = prepare_prewarm(&state).await?;
    let (generation, results) = run_prepared_prewarm(first).await;
    if launch_config_generation_is_current(generation).await {
        return Ok(results);
    }

    // A launch-config save raced the first attempt. `prepare_prewarm` retains only
    // current fingerprints before retrying, so the completed stale warm anchor
    // cannot be reused or reported as ready.
    let retry = prepare_prewarm(&state).await?;
    let (generation, results) = run_prepared_prewarm(retry).await;
    if launch_config_generation_is_current(generation).await {
        return Ok(results);
    }

    // Bound retry work while settings are changing rapidly. One final current
    // retain pass removes this attempt's stale anchors without starting more.
    let current = prepare_prewarm(&state).await?;
    let results = current
        .prepared
        .agents
        .iter()
        .map(|agent| AcpPrewarmResult {
            agent_id: agent.id.clone(),
            ready: false,
            started: false,
            error: Some("Agent launch settings changed during prewarm; retry required".into()),
        })
        .collect();
    drop(current);
    Ok(results)
}

async fn prepare_prewarm(state: &AppState) -> Result<LockedPreparedPrewarm, String> {
    let launch = load_locked_launch_config(state).await?;
    let limits = runtime_limits(&launch.file);
    let auto_approve = matches!(
        launch.file.general.permission_default.as_str(),
        "full_access" | "auto_approve"
    );
    let runtime = runtime();
    let cleanup_runtime = runtime.clone();
    let agents = overlay_enabled_agents_or_cleanup(
        &launch.file,
        &launch.proxy,
        move |agent_ids| async move {
            cleanup_runtime.drop_agent_sessions(&agent_ids).await;
        },
    )
    .await?;
    runtime
        .retain_warm_agents(&agents, limits.max_processes)
        .await;
    let LockedLaunchConfig {
        launch_generation,
        _guard: guard,
        ..
    } = launch;
    Ok(LockedPreparedPrewarm {
        prepared: PreparedPrewarm {
            agents,
            auto_approve,
            limits,
            launch_generation,
        },
        guard,
    })
}

async fn overlay_enabled_agents_or_cleanup<Cleanup, CleanupFuture>(
    file: &AcpAgentsFile,
    proxy: &ProcessProxySettings,
    cleanup: Cleanup,
) -> Result<Vec<ConfiguredAgent>, String>
where
    Cleanup: FnOnce(Vec<String>) -> CleanupFuture,
    CleanupFuture: std::future::Future<Output = ()>,
{
    let agents = enabled_agents(file)
        .into_iter()
        .cloned()
        .map(|agent| agent_with_process_proxy(agent, proxy))
        .collect::<Result<Vec<_>, _>>();
    match agents {
        Ok(agents) => Ok(agents),
        Err(error) => {
            cleanup(file.agents.iter().map(|agent| agent.id.clone()).collect()).await;
            Err(error)
        }
    }
}

async fn run_prepared_prewarm(locked: LockedPreparedPrewarm) -> (u64, Vec<AcpPrewarmResult>) {
    let LockedPreparedPrewarm { prepared, guard } = locked;
    let generation = prepared.launch_generation;
    let runtime = runtime();
    let auto_approve = prepared.auto_approve;
    let limits = prepared.limits;
    let tasks = prepared.agents.into_iter().map(|agent| {
        let runtime = runtime.clone();
        async move {
            match runtime.prewarm_agent(&agent, auto_approve, limits).await {
                Ok(started) => AcpPrewarmResult {
                    agent_id: agent.id,
                    ready: true,
                    started,
                    error: None,
                },
                Err(error) => AcpPrewarmResult {
                    agent_id: agent.id,
                    ready: false,
                    started: false,
                    error: Some(error.to_string()),
                },
            }
        }
    });
    let results = run_after_config_unlock(guard, futures::future::join_all(tasks)).await;
    (generation, results)
}

async fn run_after_config_unlock<T>(
    guard: tokio::sync::MutexGuard<'static, ()>,
    operation: impl std::future::Future<Output = T>,
) -> T {
    drop(guard);
    operation.await
}

async fn launch_config_generation_is_current(generation: u64) -> bool {
    let _guard = config_lock().lock().await;
    generation == ACP_LAUNCH_CONFIG_GENERATION.load(AtomicOrdering::SeqCst)
}

fn draft_session_key(project_id: &str, agent_id: &str) -> String {
    format!("draft:{project_id}:{agent_id}")
}

fn is_draft_session_key(session_key: &str) -> bool {
    session_key.starts_with("draft:")
}

async fn persist_live_thread_snapshot(
    db: &sea_orm::DatabaseConnection,
    thread_id: &str,
    snapshot: &AcpSessionSnapshot,
    fallback_mode_id: Option<&str>,
) -> Result<(), String> {
    let mode_id = persisted_mode_id(snapshot).or_else(|| fallback_mode_id.map(str::to_string));
    let persisted = acp_repo::persist_prepared_thread_session(
        db,
        thread_id,
        &snapshot.session_id,
        mode_id.as_deref(),
    )
    .await
    .map_err(|error| error.to_string())?;
    if persisted {
        return Ok(());
    }
    runtime().drop_session(thread_id).await;
    Err(format!(
        "ACP thread `{thread_id}` was deleted while its session was being prepared"
    ))
}

async fn schedule_capability_refresh(
    app: AppHandle,
    db: sea_orm::DatabaseConnection,
    session_key: String,
) {
    let runtime = runtime();
    let Some(handle) = runtime.capability_discovery_handle(&session_key).await else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        match handle.wait().await {
            Ok(Some((current_key, snapshot))) => {
                if !is_draft_session_key(&current_key) {
                    if let Err(error) =
                        persist_live_thread_snapshot(&db, &current_key, &snapshot, None).await
                    {
                        tracing::warn!(%error, thread_id = %current_key, "discarding late ACP capability discovery");
                        return;
                    }
                }
                if let Err(error) = app.emit(
                    "acp-session-state",
                    serde_json::json!({
                        "threadId": current_key,
                        "snapshot": snapshot,
                    }),
                ) {
                    tracing::warn!(%error, "failed to emit discovered ACP capabilities");
                }
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(%error, session_key, "ACP capability discovery refresh failed")
            }
        }
    });
}

#[cfg(test)]
mod draft_session_key_tests {
    use super::is_draft_session_key;

    #[test]
    fn only_reserved_draft_keys_skip_thread_persistence() {
        assert!(is_draft_session_key("draft:project-1:grok-build"));
        assert!(!is_draft_session_key(
            "9ca91146-52cb-44e6-a8cb-ae6df974237f"
        ));
    }
}

fn apply_launch_selection(
    mut agent: ConfiguredAgent,
    model_id: Option<&str>,
    reasoning_effort: Option<&str>,
) -> Result<ConfiguredAgent, String> {
    if let Some(model) = model_id.map(str::trim).filter(|model| !model.is_empty()) {
        agent = configured_agent_with_model(&agent, model).map_err(|error| error.to_string())?;
    }
    if let Some(effort) = reasoning_effort
        .map(str::trim)
        .filter(|effort| !effort.is_empty())
    {
        agent = configured_agent_with_reasoning_effort(&agent, effort)
            .map_err(|error| error.to_string())?;
    }
    Ok(agent)
}

#[tauri::command]
pub async fn acp_prepare_draft(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    agent_id: String,
    model_id: Option<String>,
    reasoning_effort: Option<String>,
) -> Result<AcpSessionSnapshot, String> {
    let project = acp_repo::get_project(&state.sea_db, &project_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "project not found".to_string())?;
    let launch = load_locked_launch_config(&state).await?;
    let agent = launch
        .file
        .agents
        .iter()
        .find(|agent| agent.id == agent_id && is_agent_enabled(agent))
        .cloned()
        .ok_or_else(|| format!("agent `{agent_id}` not enabled"))?;
    let agent = apply_launch_selection(agent, model_id.as_deref(), reasoning_effort.as_deref())?;
    let agent = agent_with_process_proxy(agent, &launch.proxy)?;
    let limits = runtime_limits(&launch.file);
    let auto_approve = matches!(
        launch.file.general.permission_default.as_str(),
        "full_access" | "auto_approve"
    );
    let (event_tx, _event_rx) = mpsc::unbounded_channel::<AcpEvent>();
    let session_key = draft_session_key(&project_id, &agent_id);
    let snapshot = runtime()
        .prepare(
            &session_key,
            &agent,
            PathBuf::from(project.root_path),
            None,
            auto_approve,
            limits,
            event_tx,
        )
        .await
        .map_err(|e| e.to_string())?;
    schedule_capability_refresh(app, state.sea_db.clone(), session_key).await;
    drop(launch);
    Ok(snapshot)
}

#[tauri::command]
pub async fn acp_prepare_session(
    app: AppHandle,
    state: State<'_, AppState>,
    thread_id: String,
    model_id: Option<String>,
    reasoning_effort: Option<String>,
) -> Result<AcpSessionSnapshot, String> {
    let thread = acp_repo::get_thread(&state.sea_db, &thread_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "thread not found".to_string())?;
    let project = acp_repo::get_project(&state.sea_db, &thread.project_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "project not found".to_string())?;
    let launch = load_locked_launch_config(&state).await?;
    let agent = launch
        .file
        .agents
        .iter()
        .find(|agent| agent.id == thread.agent_id && is_agent_enabled(agent))
        .cloned()
        .ok_or_else(|| format!("agent `{}` not enabled", thread.agent_id))?;
    let agent = apply_launch_selection(agent, model_id.as_deref(), reasoning_effort.as_deref())?;
    let agent = agent_with_process_proxy(agent, &launch.proxy)?;
    let limits = runtime_limits(&launch.file);
    let auto_approve = matches!(
        launch.file.general.permission_default.as_str(),
        "full_access" | "auto_approve"
    );
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<AcpEvent>();
    let thread_for_events = thread_id.clone();
    let app_for_events = app.clone();
    let event_task = tauri::async_runtime::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            match event {
                AcpEvent::SessionState { snapshot } => {
                    let _ = app_for_events.emit(
                        "acp-session-state",
                        serde_json::json!({
                            "threadId": thread_for_events,
                            "snapshot": snapshot,
                        }),
                    );
                }
                AcpEvent::Status { message } => {
                    let _ = app_for_events.emit(
                        "acp-status",
                        serde_json::json!({
                            "threadId": thread_for_events,
                            "message": message,
                            "preparing": true,
                        }),
                    );
                }
                _ => {}
            }
        }
    });

    let runtime = runtime();
    let mut snapshot = runtime
        .prepare(
            &thread_id,
            &agent,
            PathBuf::from(project.root_path),
            thread.acp_session_id.clone(),
            auto_approve,
            limits,
            event_tx,
        )
        .await
        .map_err(|e| e.to_string())?;
    if let Some(saved_mode) = thread.mode_id.as_deref() {
        match runtime
            .restore_persisted_mode(&thread_id, saved_mode)
            .await
            .map_err(|error| format!("failed to restore ACP mode `{saved_mode}`: {error}"))?
        {
            Some(restored) => snapshot = restored,
            None => {
                tracing::warn!(
                    thread_id = %thread_id,
                    mode_id = %saved_mode,
                    "clearing an ACP session mode that the agent no longer advertises"
                );
            }
        }
    }
    event_task
        .await
        .map_err(|error| format!("ACP prepare event forwarder failed: {error}"))?;
    persist_live_thread_snapshot(&state.sea_db, &thread_id, &snapshot, None).await?;
    schedule_capability_refresh(app, state.sea_db.clone(), thread_id).await;
    drop(launch);
    Ok(snapshot)
}

#[tauri::command]
pub async fn acp_set_config_option(
    state: State<'_, AppState>,
    thread_id: String,
    config_id: String,
    value: serde_json::Value,
) -> Result<AcpSessionSnapshot, String> {
    let snapshot = runtime()
        .set_config_option(&thread_id, &config_id, value)
        .await
        .map_err(|e| e.to_string())?;
    if !is_draft_session_key(&thread_id) {
        persist_live_thread_snapshot(&state.sea_db, &thread_id, &snapshot, None).await?;
    }
    Ok(snapshot)
}

#[tauri::command]
pub async fn acp_set_mode(
    state: State<'_, AppState>,
    thread_id: String,
    mode_id: String,
) -> Result<AcpSessionSnapshot, String> {
    let snapshot = runtime()
        .set_mode(&thread_id, &mode_id)
        .await
        .map_err(|e| e.to_string())?;
    if !is_draft_session_key(&thread_id) {
        persist_live_thread_snapshot(&state.sea_db, &thread_id, &snapshot, Some(&mode_id)).await?;
    }
    Ok(snapshot)
}

#[tauri::command]
pub async fn acp_cancel(state: State<'_, AppState>, thread_id: String) -> Result<bool, String> {
    let cancelled = runtime()
        .cancel(&thread_id)
        .await
        .map_err(|e| e.to_string())?;
    if cancelled {
        return Ok(true);
    }
    let interrupted = acp_repo::interrupt_streaming_messages(
        &state.sea_db,
        &thread_id,
        "The Agent process is no longer running",
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok(interrupted > 0)
}
