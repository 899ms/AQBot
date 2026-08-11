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

    /// Resolve an outstanding interaction through ACP's native cancelled
    /// outcome without inventing an option id that the agent did not offer.
    pub async fn cancel_interaction(&self, request_id: &str) -> bool {
        let pending = self.permissions.lock().await.remove(request_id);
        let Some(pending) = pending else {
            return false;
        };
        let PendingPermission {
            interaction_kind,
            tool_call_id,
            event_tx,
            sender,
            ..
        } = pending;
        // Closing the pending response channel makes the protocol-specific
        // handler return its native cancelled response to the agent.
        drop(sender);
        emit_interaction_closed(
            &event_tx,
            request_id,
            interaction_kind,
            tool_call_id,
            AcpInteractionOutcome::Cancelled,
            None,
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
            let questionnaire = pending
                .questionnaire
                .as_mut()
                .ok_or_else(|| "interaction is not a questionnaire".to_string())?;
            let outcome = submission.outcome;
            let summary = match questionnaire {
                PendingQuestionnaire::Grok { context, sender } => {
                    let summary = validate_questionnaire_submission(context, &submission)?;
                    sender
                        .take()
                        .ok_or_else(|| "questionnaire was already resolved".to_string())?
                        .send(submission)
                        .map_err(|_| {
                            "questionnaire responder is no longer available".to_string()
                        })?;
                    summary
                }
                PendingQuestionnaire::Elicitation { context, sender } => {
                    let (summary, response) =
                        elicitation_response_from_submission(context, &submission)?;
                    sender
                        .take()
                        .ok_or_else(|| "questionnaire was already resolved".to_string())?
                        .send(response)
                        .map_err(|_| {
                            "questionnaire responder is no longer available".to_string()
                        })?;
                    summary
                }
                PendingQuestionnaire::Qwen { context, sender } => {
                    let (summary, response) = qwen_response_from_submission(context, &submission)?;
                    sender
                        .take()
                        .ok_or_else(|| "questionnaire was already resolved".to_string())?
                        .send(response)
                        .map_err(|_| {
                            "questionnaire responder is no longer available".to_string()
                        })?;
                    summary
                }
            };
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
            AcpQuestionnaireOutcome::Declined => "declined",
            AcpQuestionnaireOutcome::ChatAboutThis => "chat_about_this",
            AcpQuestionnaireOutcome::SkipInterview => "skip_interview",
            AcpQuestionnaireOutcome::Cancelled => "cancelled",
        };
        let selected = matches!(terminal_outcome, AcpInteractionOutcome::Selected).then(|| {
            PermissionOptionView {
                option_id: option_id.into(),
                name: summary.clone(),
                kind: None,
                description: None,
            }
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

    /// Detach local runtime state without changing the remote ACP session.
    pub async fn drop_session(&self, session_key: &str) {
        let removed = self.sessions.lock().await.remove(session_key);
        if let Some(live) = removed {
            unregister_live_route(&live).await;
            self.cancel_permissions(&live.permission_scope).await;
        }
    }

    /// Close a user-deleted ACP session when supported, then detach its local state.
    pub async fn close_session(&self, session_key: &str) -> anyhow::Result<bool> {
        let lifecycle = self.session_lifecycle_lock(session_key).await;
        let _lifecycle = lifecycle.lock().await;
        let Some(live) = self.sessions.lock().await.get(session_key).cloned() else {
            return Ok(false);
        };
        let _admission = live.admission_lock.lock().await;
        if live.prompt_state.load(Ordering::Acquire) != PROMPT_IDLE {
            anyhow::bail!("cannot close an ACP session while a prompt is running");
        }
        let _operation = live.operation_lock.lock().await;
        let _process = live.process_operation_lock.lock().await;
        if live.prompt_state.load(Ordering::Acquire) != PROMPT_IDLE {
            anyhow::bail!("cannot close an ACP session while a prompt is running");
        }
        if !self
            .sessions
            .lock()
            .await
            .get(session_key)
            .is_some_and(|current| current.permission_scope == live.permission_scope)
        {
            return Ok(false);
        }

        let session_id = { live.active.lock().await.id.clone() };
        if let Some(session_id) = session_id {
            let metadata = live_metadata(&live).await?;
            if metadata.capabilities.session_capabilities.close.is_some() {
                let connection = live_connection(&live).await?;
                self.live_control_request(
                    &live,
                    "session/close",
                    connection
                        .send_request(CloseSessionRequest::new(session_id))
                        .block_task(),
                )
                .await?;
            }
        }

        let removed = {
            let mut sessions = self.sessions.lock().await;
            if sessions
                .get(session_key)
                .is_some_and(|current| current.permission_scope == live.permission_scope)
            {
                sessions.remove(session_key)
            } else {
                None
            }
        };
        let Some(removed) = removed else {
            return Ok(false);
        };
        unregister_live_route(&removed).await;
        self.cancel_permissions(&removed.permission_scope).await;
        Ok(true)
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
        let session_control_timed_out = result
            .as_ref()
            .err()
            .is_some_and(is_session_control_timeout);
        let drain_result = drain_notification_work(&live.notification_barrier_tx).await;
        *live.event_slot.lock().await = None;
        live.touch();
        let outcome = match (result, drain_result) {
            (Ok(snapshot), Ok(())) => Ok(snapshot),
            (Err(error), Ok(())) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Err(error), Err(drain_error)) => Err(anyhow::anyhow!(
                "{error}; ACP notification drain also failed: {drain_error}"
            )),
        };
        if session_control_timed_out {
            drop(_busy);
            drop(_operation);
            self.shutdown_process_scope(&live).await;
        }
        outcome
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
            self.live_control_request(
                &live,
                "session/set_model",
                connection
                    .send_request(LegacySetModelRequest::new(session_id.clone(), model_id))
                    .block_task(),
            )
            .await?;
            let mut active = live.active.lock().await;
            apply_legacy_model_selection(
                &mut active.config_options,
                metadata.meta.as_ref(),
                model_id,
            );
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
            self.live_control_request(
                &live,
                "session/set_model_reasoning",
                connection
                    .send_request(LegacySetModelRequest::with_reasoning(
                        session_id.clone(),
                        &model_id,
                        reasoning_effort,
                    ))
                    .block_task(),
            )
            .await?;
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
            let response = self
                .live_control_request(
                    &live,
                    "session/set_config_option",
                    connection
                        .send_request(SetSessionConfigOptionRequest::new(
                            session_id,
                            config_id.to_string(),
                            option_value,
                        ))
                        .block_task(),
                )
                .await?;
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
        self.live_control_request(
            &live,
            "session/set_mode",
            connection
                .send_request(SetSessionModeRequest::new(
                    session_id,
                    SessionModeId::new(mode_id),
                ))
                .block_task(),
        )
        .await?;
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

    async fn live_control_request<T, E>(
        &self,
        live: &LiveSession,
        method: &'static str,
        request: impl Future<Output = Result<T, E>>,
    ) -> anyhow::Result<T>
    where
        E: std::fmt::Display,
    {
        let result =
            session_control_request(method, live_session_control_timeout(live)?, request).await;
        if result
            .as_ref()
            .err()
            .is_some_and(is_session_control_timeout)
        {
            self.shutdown_process_scope(live).await;
        }
        result
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
        let session_lock = self.session_lifecycle_lock(session_key).await;
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

    async fn session_lifecycle_lock(&self, session_key: &str) -> Arc<Mutex<()>> {
        let mut locks = self.session_locks.lock().await;
        locks
            .entry(session_key.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
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
