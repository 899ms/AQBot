// ACP prompt input, event forwarding, and interaction commands.

fn attachment_file_uri(
    file_store: &aqbot_core::file_store::FileStore,
    attachment: &Attachment,
) -> Result<String, String> {
    let path = file_store
        .validated_path(&attachment.file_path)
        .map_err(|error| {
            format!(
                "Invalid persisted attachment path for {}: {error}",
                attachment.file_name
            )
        })?;
    reqwest::Url::from_file_path(&path)
        .map(|url| url.to_string())
        .map_err(|_| {
            format!(
                "Could not convert persisted attachment path to a file URI: {}",
                path.display()
            )
        })
}

fn build_prompt_input(
    text: String,
    inputs: &[AttachmentInput],
    persisted: &[Attachment],
) -> Result<AcpPromptInput, String> {
    build_prompt_input_with_store(
        text,
        inputs,
        persisted,
        &aqbot_core::file_store::FileStore::new(),
    )
}

fn build_prompt_input_with_store(
    text: String,
    inputs: &[AttachmentInput],
    persisted: &[Attachment],
    file_store: &aqbot_core::file_store::FileStore,
) -> Result<AcpPromptInput, String> {
    if inputs.len() != persisted.len() {
        return Err(format!(
            "Persisted attachment count mismatch: expected {}, got {}",
            inputs.len(),
            persisted.len()
        ));
    }
    let attachments = inputs
        .iter()
        .zip(persisted)
        .map(|(input, attachment)| {
            let mime_type = aqbot_core::storage_paths::normalize_attachment_mime_type(
                &attachment.file_name,
                &attachment.file_type,
            );
            let is_image = aqbot_core::storage_paths::is_image_mime_type(&mime_type);
            Ok(AcpPromptAttachment {
                file_name: attachment.file_name.clone(),
                mime_type,
                file_size: attachment.file_size,
                data: is_image.then(|| input.data.clone()),
                file_uri: attachment_file_uri(file_store, attachment)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(AcpPromptInput { text, attachments })
}

async fn rollback_prompt_receipt(
    db: &sea_orm::DatabaseConnection,
    thread_id: &str,
    user_message_id: &str,
    assistant_message_id: &str,
    primary: String,
) -> String {
    let ids = vec![
        user_message_id.to_string(),
        assistant_message_id.to_string(),
    ];
    match acp_repo::rollback_prompt_messages(db, thread_id, &ids).await {
        Ok(()) => primary,
        Err(error) => format!("{primary}; ACP prompt rollback failed: {error}"),
    }
}

#[cfg(test)]
mod prompt_input_tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn prompt_input_uses_persisted_file_uris_and_keeps_base64_only_for_images() {
        let root = tempfile::tempdir().unwrap();
        let store = aqbot_core::file_store::FileStore::with_root(root.path().to_path_buf());
        let image_bytes = b"image";
        let file_bytes = b"document";
        let saved_image = store
            .save_file(image_bytes, "my image.png", "image/png")
            .unwrap();
        let saved_file = store
            .save_file(file_bytes, "notes #1.txt", "text/plain")
            .unwrap();
        let inputs = vec![
            AttachmentInput {
                file_name: "my image.png".to_string(),
                file_type: "application/x-custom".to_string(),
                file_size: image_bytes.len() as u64,
                data: base64::engine::general_purpose::STANDARD.encode(image_bytes),
            },
            AttachmentInput {
                file_name: "notes #1.txt".to_string(),
                file_type: "text/plain".to_string(),
                file_size: file_bytes.len() as u64,
                data: base64::engine::general_purpose::STANDARD.encode(file_bytes),
            },
        ];
        let persisted = vec![
            Attachment {
                id: "image-id".to_string(),
                file_type: "application/x-custom".to_string(),
                file_name: "my image.png".to_string(),
                file_path: saved_image.storage_path.clone(),
                file_size: image_bytes.len() as u64,
                data: None,
            },
            Attachment {
                id: "file-id".to_string(),
                file_type: "text/plain".to_string(),
                file_name: "notes #1.txt".to_string(),
                file_path: saved_file.storage_path.clone(),
                file_size: file_bytes.len() as u64,
                data: None,
            },
        ];

        let prompt =
            build_prompt_input_with_store("inspect".to_string(), &inputs, &persisted, &store)
                .unwrap();

        assert_eq!(
            prompt.attachments[0].data.as_deref(),
            Some(inputs[0].data.as_str())
        );
        assert_eq!(prompt.attachments[0].mime_type, "image/png");
        assert!(prompt.attachments[1].data.is_none());
        for (prepared, metadata) in prompt.attachments.iter().zip(&persisted) {
            let url = reqwest::Url::parse(&prepared.file_uri).unwrap();
            assert_eq!(url.scheme(), "file");
            assert_eq!(
                url.to_file_path().unwrap(),
                store.resolve_path(&metadata.file_path)
            );
        }
    }
}

#[tauri::command]
pub async fn acp_prompt(
    app: AppHandle,
    state: State<'_, AppState>,
    thread_id: String,
    prompt: String,
    attachments: Option<Vec<AttachmentInput>>,
    model_id: Option<String>,
    reasoning_effort: Option<String>,
) -> Result<AcpPromptAccepted, String> {
    let attachments = attachments.unwrap_or_default();
    if prompt.trim().is_empty() && attachments.is_empty() {
        return Err("prompt must contain text or attachments".into());
    }
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
    let cwd = PathBuf::from(&project.root_path);
    let rt = runtime();

    // Initialization is both a launch preflight and the authoritative source
    // for image capability. Do it before writing files or messages.
    let (prepare_tx, _prepare_rx) = mpsc::unbounded_channel::<AcpEvent>();
    let snapshot = rt
        .prepare(
            &thread_id,
            &agent,
            cwd.clone(),
            thread.acp_session_id.clone(),
            auto_approve,
            limits,
            prepare_tx,
        )
        .await
        .map_err(|error| error.to_string())?;
    if attachments.iter().any(|attachment| {
        aqbot_core::storage_paths::is_image_attachment(&attachment.file_name, &attachment.file_type)
    }) && !snapshot.agent_capabilities.prompt_capabilities.image
    {
        return Err("ACP agent does not advertise image prompt capability".to_string());
    }
    persist_live_thread_snapshot(&state.sea_db, &thread_id, &snapshot, None).await?;

    let (user_message, assistant) =
        acp_repo::create_prompt_messages(&state.sea_db, &thread_id, &prompt, &attachments)
            .await
            .map_err(|error| error.to_string())?;
    let prompt_input = match build_prompt_input(prompt, &attachments, &user_message.attachments) {
        Ok(input) => input,
        Err(error) => {
            return Err(rollback_prompt_receipt(
                &state.sea_db,
                &thread_id,
                &user_message.id,
                &assistant.id,
                error,
            )
            .await)
        }
    };

    let session_id = Some(snapshot.session_id);
    let db = state.sea_db.clone();
    let assistant_id = assistant.id.clone();
    let thread_id_clone = thread_id.clone();

    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<AcpEvent>();
    let accumulated_text = Arc::new(Mutex::new(String::new()));
    let tool_transcript = Arc::new(Mutex::new(HashMap::<String, PersistedAcpToolCall>::new()));
    let turn_started = std::time::Instant::now();

    // Forward events to frontend.
    // Tool calls are also injected as inline <tool-call> markers into the
    // assistant message text (same pattern as chat agent mode) so they appear
    // in chronological order inside the bubble — not dumped under the thread.
    let app_fwd = app.clone();
    let db_for_events = db.clone();
    let thread_for_events = thread_id.clone();
    let assistant_for_events = assistant_id.clone();
    let acc_for_events = accumulated_text.clone();
    let tools_for_events = tool_transcript.clone();
    let event_task = tauri::async_runtime::spawn(async move {
        let mut thinking_open = false;
        let mut next_tool_sequence = 0_u64;
        while let Some(ev) = event_rx.recv().await {
            match &ev {
                AcpEvent::StreamText { text } => {
                    let display_text = if thinking_open {
                        thinking_open = false;
                        format!("\n</think>\n\n{text}")
                    } else {
                        text.clone()
                    };
                    {
                        let mut acc = acc_for_events.lock().await;
                        acc.push_str(&display_text);
                    }
                    let _ = app_fwd.emit(
                        "acp-stream-text",
                        serde_json::json!({
                            "threadId": thread_for_events,
                            "messageId": assistant_for_events,
                            "text": display_text,
                        }),
                    );
                }
                AcpEvent::StreamThinking { thinking } => {
                    let display_text = if thinking_open {
                        thinking.clone()
                    } else {
                        thinking_open = true;
                        format!("<think>\n{thinking}")
                    };
                    {
                        let mut acc = acc_for_events.lock().await;
                        acc.push_str(&display_text);
                    }
                    let _ = app_fwd.emit(
                        "acp-stream-text",
                        serde_json::json!({
                            "threadId": thread_for_events,
                            "messageId": assistant_for_events,
                            "text": display_text,
                        }),
                    );
                }
                AcpEvent::PermissionRequest {
                    request_id,
                    interaction_kind,
                    tool_call_id,
                    title,
                    raw,
                    options,
                } => {
                    // Plan reviews are injected as inline markers so the card
                    // stays mid-message (before any later assistant text). Full
                    // plan body is embedded so reloads can rehydrate the card.
                    if matches!(interaction_kind, AcpInteractionKind::PlanReview) {
                        let plan_body = extract_plan_content_from_raw(raw)
                            .or_else(|| {
                                title
                                    .as_ref()
                                    .map(|s| s.trim().to_string())
                                    .filter(|s| !s.is_empty())
                            })
                            .unwrap_or_default();
                        let plan_title = title.clone().or_else(|| {
                            raw.get("title")
                                .and_then(|v| v.as_str())
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                                .map(str::to_string)
                        });
                        let marker = if thinking_open {
                            thinking_open = false;
                            format!(
                                "\n</think>\n\n{}",
                                build_acp_plan_marker(
                                    request_id,
                                    &assistant_for_events,
                                    &plan_title,
                                    &plan_body,
                                    "pending",
                                )
                            )
                        } else {
                            build_acp_plan_marker(
                                request_id,
                                &assistant_for_events,
                                &plan_title,
                                &plan_body,
                                "pending",
                            )
                        };
                        let marker_id = format!(
                            "<acp-plan data-aqbot=\"1\" id=\"{}\"",
                            xml_attr_escape(request_id)
                        );
                        let should_emit_marker = {
                            let mut acc = acc_for_events.lock().await;
                            if acc.contains(&marker_id) {
                                false
                            } else {
                                acc.push_str(&marker);
                                true
                            }
                        };
                        if should_emit_marker {
                            let _ = app_fwd.emit(
                                "acp-stream-text",
                                serde_json::json!({
                                    "threadId": thread_for_events,
                                    "messageId": assistant_for_events,
                                    "text": marker,
                                }),
                            );
                        }
                    }
                    let _ = app_fwd.emit(
                        "acp-permission-request",
                        serde_json::json!({
                            "threadId": thread_for_events,
                            "messageId": assistant_for_events,
                            "requestId": request_id,
                            "interactionKind": interaction_kind,
                            "toolCallId": tool_call_id,
                            "title": title,
                            "raw": raw,
                            "options": options,
                        }),
                    );
                }
                AcpEvent::InteractionClosed {
                    request_id,
                    interaction_kind,
                    tool_call_id,
                    outcome,
                    selected_option_id,
                    selected_option_kind,
                    selected_option_name,
                } => {
                    if let Some(tool_call_id) = tool_call_id {
                        let mut tools = tools_for_events.lock().await;
                        record_interaction_outcome(
                            &mut tools,
                            &mut next_tool_sequence,
                            tool_call_id,
                            *interaction_kind,
                            *outcome,
                            selected_option_id.as_deref(),
                            selected_option_kind.as_deref(),
                            selected_option_name.as_deref(),
                        );
                    }
                    // Persist final plan-review outcome on the inline marker so
                    // a refresh still shows approved/cancelled/abandoned.
                    if matches!(interaction_kind, AcpInteractionKind::PlanReview) {
                        let status = plan_review_status_from_outcome(
                            *outcome,
                            selected_option_id.as_deref(),
                        );
                        let mut acc = acc_for_events.lock().await;
                        let _ = patch_acp_plan_marker_status(&mut acc, request_id, status);
                    }
                    let _ = app_fwd.emit(
                        "acp-interaction-closed",
                        serde_json::json!({
                            "threadId": thread_for_events,
                            "messageId": assistant_for_events,
                            "requestId": request_id,
                            "interactionKind": interaction_kind,
                            "toolCallId": tool_call_id,
                            "reason": outcome,
                            "selectedOptionId": selected_option_id,
                            "selectedOptionKind": selected_option_kind,
                            "selectedOptionName": selected_option_name,
                        }),
                    );
                }
                AcpEvent::ToolCall {
                    tool_call_id,
                    title,
                    kind,
                    status,
                    raw,
                } => {
                    {
                        let mut tools = tools_for_events.lock().await;
                        record_tool_call(
                            &mut tools,
                            &mut next_tool_sequence,
                            tool_call_id,
                            title,
                            kind,
                            status,
                            raw,
                        );
                    }
                    // Chronological inline marker → stream + DB final text
                    let marker = if thinking_open {
                        thinking_open = false;
                        format!(
                            "\n</think>\n\n{}",
                            build_acp_tool_call_marker(
                                tool_call_id,
                                &assistant_for_events,
                                title,
                                kind,
                                raw,
                            )
                        )
                    } else {
                        build_acp_tool_call_marker(
                            tool_call_id,
                            &assistant_for_events,
                            title,
                            kind,
                            raw,
                        )
                    };
                    let id_attr = format!("id=\"{}\"", xml_attr_escape(tool_call_id));
                    let should_emit_marker = {
                        let mut acc = acc_for_events.lock().await;
                        if acc.contains(&id_attr) {
                            false
                        } else {
                            acc.push_str(&marker);
                            true
                        }
                    };
                    if should_emit_marker {
                        let _ = app_fwd.emit(
                            "acp-stream-text",
                            serde_json::json!({
                                "threadId": thread_for_events,
                                "messageId": assistant_for_events,
                                "text": marker,
                            }),
                        );
                    }
                    let _ = app_fwd.emit(
                        "acp-tool-call",
                        serde_json::json!({
                            "threadId": thread_for_events,
                            "messageId": assistant_for_events,
                            "toolCallId": tool_call_id,
                            "title": title,
                            "kind": kind,
                            "status": status,
                            "raw": raw,
                        }),
                    );
                }
                AcpEvent::ToolCallUpdate {
                    tool_call_id,
                    status,
                    raw,
                } => {
                    {
                        let mut tools = tools_for_events.lock().await;
                        let sequence = tools.get(tool_call_id).map_or_else(
                            || {
                                let current = next_tool_sequence;
                                next_tool_sequence += 1;
                                current
                            },
                            |tool| tool.sequence,
                        );
                        let tool = tools.entry(tool_call_id.clone()).or_insert_with(|| {
                            PersistedAcpToolCall {
                                tool_call_id: tool_call_id.clone(),
                                tool_name: "tool".into(),
                                status: "running".into(),
                                input: None,
                                output: None,
                                approval_status: None,
                                approval_option_id: None,
                                approval_option_kind: None,
                                approval_label: None,
                                sequence,
                            }
                        });
                        if let Some(status) = status {
                            tool.status = status.clone();
                        }
                        if let Some(input) = tool_input_detail(raw) {
                            tool.input = Some(input);
                        }
                        if let Some(output) = tool_output_detail(raw) {
                            tool.output = Some(output);
                        }
                    }
                    let _ = app_fwd.emit(
                        "acp-tool-call-update",
                        serde_json::json!({
                            "threadId": thread_for_events,
                            "messageId": assistant_for_events,
                            "toolCallId": tool_call_id,
                            "status": status,
                            "raw": raw,
                        }),
                    );
                }
                AcpEvent::Plan { raw } => {
                    let _ = app_fwd.emit(
                        "acp-plan",
                        serde_json::json!({
                            "threadId": thread_for_events,
                            "messageId": assistant_for_events,
                            "raw": raw,
                        }),
                    );
                }
                AcpEvent::SessionState { snapshot } => {
                    let mode_id = persisted_mode_id(snapshot);
                    if let Err(error) = acp_repo::update_thread_mode(
                        &db_for_events,
                        &thread_for_events,
                        mode_id.as_deref(),
                    )
                    .await
                    {
                        tracing::error!(%error, thread_id = %thread_for_events, "failed to persist ACP session mode update");
                    }
                    let _ = app_fwd.emit(
                        "acp-session-state",
                        serde_json::json!({
                            "threadId": thread_for_events,
                            "snapshot": snapshot,
                        }),
                    );
                }
                AcpEvent::Status { message } => {
                    let _ = app_fwd.emit(
                        "acp-status",
                        serde_json::json!({
                            "threadId": thread_for_events,
                            "message": message,
                        }),
                    );
                }
                AcpEvent::Error { message } => {
                    let _ = app_fwd.emit(
                        "acp-status",
                        serde_json::json!({
                            "threadId": thread_for_events,
                            "message": message,
                        }),
                    );
                }
                // Runtime emits this only after session/prompt has returned and
                // notification routing has been detached. It is the explicit
                // drain boundary; UI finalization still happens after DB persist.
                AcpEvent::Done { .. } => break,
            }
        }
        if thinking_open {
            let close = "\n</think>\n";
            acc_for_events.lock().await.push_str(close);
            let _ = app_fwd.emit(
                "acp-stream-text",
                serde_json::json!({
                    "threadId": thread_for_events,
                    "messageId": assistant_for_events,
                    "text": close,
                }),
            );
        }
    });

    // `schedule_prompt` is the acceptance boundary: initialization, capability
    // conversion, busy checks, and worker enqueue all complete before the IPC
    // command returns. Any failure here rolls back the just-created receipt.
    let prompt_handle = match rt
        .schedule_prompt(
            &thread_id,
            &agent,
            cwd,
            prompt_input,
            session_id,
            auto_approve,
            limits,
            event_tx,
        )
        .await
    {
        Ok(handle) => handle,
        Err(error) => {
            if let Err(join_error) = event_task.await {
                tracing::warn!(%join_error, thread_id = %thread_id, "ACP event forwarder failed after scheduling rejection");
            }
            return Err(rollback_prompt_receipt(
                &state.sea_db,
                &thread_id,
                &user_message.id,
                &assistant.id,
                error.to_string(),
            )
            .await);
        }
    };
    // The turn is now active, so a concurrent launch-config save preserves it while
    // invalidating only idle sessions and warm anchors for the next turn.
    drop(launch);

    let acc_for_persist = accumulated_text.clone();
    let tools_for_persist = tool_transcript.clone();
    tauri::async_runtime::spawn(async move {
        let result = prompt_handle.wait().await;

        // The channel closes after the worker clears its per-turn sender. Waiting
        // drains every already-delivered notification without an arbitrary sleep.
        if let Err(error) = event_task.await {
            tracing::error!(%error, thread_id = %thread_id_clone, "ACP event forwarder failed");
        }
        let final_text = acc_for_persist.lock().await.clone();
        let duration_ms = turn_started.elapsed().as_millis() as u64;
        let terminal_tool_status = match result.as_ref() {
            Ok(outcome) if outcome.stop_reason.to_ascii_lowercase().contains("cancel") => {
                "cancelled"
            }
            Ok(_) | Err(_) => "error",
        };
        let mut tools = tools_for_persist.lock().await;
        finalize_unfinished_tool_calls(&mut tools, terminal_tool_status);
        let mut tool_calls = tools.values().cloned().collect::<Vec<_>>();
        drop(tools);
        tool_calls.sort_by_key(|tool| tool.sequence);
        let meta = serde_json::json!({
            "duration_ms": duration_ms,
            "toolCalls": tool_calls,
        })
        .to_string();

        match result {
            Ok(outcome) => {
                let persist_result = acp_repo::finalize_prompt(
                    &db,
                    acp_repo::AcpPromptFinalization {
                        thread_id: &thread_id_clone,
                        message_id: &assistant_id,
                        content: &final_text,
                        message_status: "done",
                        meta_json: Some(&meta),
                        acp_session_id: Some(&outcome.session_id),
                        runtime_status: "idle",
                    },
                )
                .await
                .map_err(|error| error.to_string());
                if let Err(error) = persist_result {
                    tracing::error!(%error, thread_id = %thread_id_clone, "failed to persist completed ACP turn");
                    runtime().drop_session(&thread_id_clone).await;
                    if let Err(emit_error) = app.emit(
                        "acp-error",
                        serde_json::json!({
                            "threadId": &thread_id_clone,
                            "messageId": &assistant_id,
                            "message": format!("Failed to persist ACP response: {error}"),
                            "text": final_text,
                            "durationMs": duration_ms,
                        }),
                    ) {
                        tracing::warn!(%emit_error, thread_id = %thread_id_clone, "failed to emit ACP persistence error");
                    }
                    return;
                }
                // Emit AFTER DB write so any subsequent loadMessages sees status=done.
                if let Err(error) = app.emit(
                    "acp-done",
                    serde_json::json!({
                        "threadId": &thread_id_clone,
                        "messageId": &assistant_id,
                        "stopReason": outcome.stop_reason,
                        "sessionId": outcome.session_id,
                        "text": final_text,
                        "durationMs": duration_ms,
                    }),
                ) {
                    tracing::warn!(%error, thread_id = %thread_id_clone, "failed to emit acp-done");
                }
            }
            Err(e) => {
                let err_text = if final_text.is_empty() {
                    format!("Error: {e}")
                } else {
                    format!("{final_text}\n\nError: {e}")
                };
                if let Err(error) = acp_repo::finalize_prompt(
                    &db,
                    acp_repo::AcpPromptFinalization {
                        thread_id: &thread_id_clone,
                        message_id: &assistant_id,
                        content: &err_text,
                        message_status: "error",
                        meta_json: Some(&meta),
                        acp_session_id: None,
                        runtime_status: "error",
                    },
                )
                .await
                {
                    tracing::error!(%error, thread_id = %thread_id_clone, "failed to persist ACP error state");
                }
                if let Err(error) = app.emit(
                    "acp-error",
                    serde_json::json!({
                        "threadId": &thread_id_clone,
                        "messageId": &assistant_id,
                        "message": e.to_string(),
                        "text": err_text,
                        "durationMs": duration_ms,
                    }),
                ) {
                    tracing::warn!(%error, thread_id = %thread_id_clone, "failed to emit acp-error");
                }
            }
        }
    });

    Ok(AcpPromptAccepted {
        user_message,
        assistant_message: assistant,
    })
}

#[tauri::command]
pub async fn acp_respond_permission(
    request_id: String,
    option_id: String,
    feedback: Option<String>,
) -> Result<(), String> {
    if runtime()
        .resolve_permission(&request_id, option_id, feedback)
        .await
    {
        Ok(())
    } else {
        Err("permission request not found or already resolved".into())
    }
}

#[tauri::command]
pub async fn acp_cancel_interaction(request_id: String) -> Result<(), String> {
    if runtime().cancel_interaction(&request_id).await {
        Ok(())
    } else {
        Err("interaction not found or already resolved".into())
    }
}

#[tauri::command]
pub async fn acp_respond_questionnaire(
    request_id: String,
    outcome: AcpQuestionnaireOutcome,
    answers: Vec<AcpQuestionnaireAnswer>,
) -> Result<String, String> {
    runtime()
        .resolve_questionnaire(&request_id, AcpQuestionnaireSubmission { outcome, answers })
        .await
}

/// Debug helper: registry source label for UI.
#[tauri::command]
pub async fn acp_registry_source() -> Result<String, String> {
    let reg = load_registry().map_err(|e| e.to_string())?;
    Ok(match reg.source.unwrap_or(RegistrySource::Builtin) {
        RegistrySource::Builtin => "builtin".into(),
        RegistrySource::Cache => "cache".into(),
        RegistrySource::Live => "live".into(),
    })
}
