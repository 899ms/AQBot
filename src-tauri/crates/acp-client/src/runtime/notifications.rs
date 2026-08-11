async fn resolve_session_route(routes: &RouteMap, session_id: &SessionId) -> Option<SessionRoute> {
    let session_id = session_id.to_string();
    routes.lock().await.by_session_id.get(&session_id).cloned()
}

async fn register_session_route(routes: &RouteMap, session_id: &SessionId, route: &SessionRoute) {
    let mut routes = routes.lock().await;
    let session_id = session_id.to_string();
    routes
        .by_session_id
        .retain(|_, existing| existing.permission_scope != route.permission_scope);
    routes
        .by_session_id
        .insert(session_id.clone(), route.clone());
    if routes
        .opening
        .as_ref()
        .is_some_and(|opening| opening.permission_scope == route.permission_scope)
    {
        routes.opening = None;
        let matching = routes
            .pending_notifications
            .remove(&session_id)
            .unwrap_or_default();
        routes.pending_notifications.clear();
        routes.routed_notifications.extend(
            matching
                .into_iter()
                .map(|notification| (route.clone(), notification)),
        );
    }
}

async fn route_session_notification(
    notification: SessionNotification,
    routes: &RouteMap,
    metadata: &Arc<Mutex<Option<AgentMetadata>>>,
) {
    let session_id = notification.session_id.to_string();
    let route = {
        let mut routes = routes.lock().await;
        if let Some(route) = routes.by_session_id.get(&session_id) {
            Some(route.clone())
        } else if routes.opening.is_some() {
            routes
                .pending_notifications
                .entry(session_id.clone())
                .or_default()
                .push(notification);
            return;
        } else {
            None
        }
    };
    let Some(route) = route else {
        tracing::warn!(
            session_id,
            "ignoring ACP update for an unknown logical session"
        );
        return;
    };
    emit_session_notification(notification, route, metadata).await;
}

async fn flush_routed_session_notifications(
    routes: &RouteMap,
    metadata: &Arc<Mutex<Option<AgentMetadata>>>,
) {
    let notifications = std::mem::take(&mut routes.lock().await.routed_notifications);
    for (route, notification) in notifications {
        emit_session_notification(notification, route, metadata).await;
    }
}

async fn emit_session_notification(
    notification: SessionNotification,
    route: SessionRoute,
    metadata: &Arc<Mutex<Option<AgentMetadata>>>,
) {
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
