async fn prepare_live_session(
    live: &LiveSession,
    preferred_session_id: Option<&str>,
    event_tx: &mpsc::UnboundedSender<AcpEvent>,
) -> anyhow::Result<AcpSessionSnapshot> {
    let connection = live_connection(live).await?;
    let metadata = live_metadata(live).await?;
    let session_control_timeout = live_session_control_timeout(live)?;
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
        session_control_timeout,
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
    session_control_timeout: Duration,
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
    {
        let mut routes = routes.lock().await;
        routes.pending_notifications.clear();
        routes.opening = Some(route.clone());
    }
    let result = ensure_agent_session(
        connection,
        cwd,
        preferred_session_id,
        metadata,
        active,
        event_tx,
        session_control_timeout,
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
                routes.pending_notifications.clear();
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
    session_control_timeout: Duration,
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
            match session_control_request(
                "session/load",
                session_control_timeout,
                connection
                    .send_request(LoadSessionRequest::new(preferred_id.clone(), cwd.clone()))
                    .block_task(),
            )
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
                        return Err(error);
                    }
                    tracing::warn!(%error, session = preferred, "saved ACP session is missing");
                    let _ = event_tx.send(AcpEvent::Status {
                        message: ACP_STATUS_SAVED_SESSION_EXPIRED.into(),
                    });
                }
            }
        } else if metadata.capabilities.session_capabilities.resume.is_some() {
            match session_control_request(
                "session/resume",
                session_control_timeout,
                connection
                    .send_request(ResumeSessionRequest::new(preferred_id.clone(), cwd.clone()))
                    .block_task(),
            )
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
                        return Err(error);
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
    let response = session_control_request(
        "session/new",
        session_control_timeout,
        connection
            .send_request(ExtendedNewSessionRequest::new(cwd.clone()))
            .block_task(),
    )
    .await?;
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
    session_control_timeout: Duration,
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
        session_control_timeout,
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
                    session_control_timeout,
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
