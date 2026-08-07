//! ACP workbench Tauri commands.

use crate::AppState;
use aqbot_acp_client::config::{
    enabled_agents, load_agents_file, probe_agent, remove_agent, reorder_agents, save_agents_file,
    set_agent_enabled, upsert_from_registry, AcpAgentsFile, AcpGeneralConfig, ConfiguredAgent,
};
use aqbot_acp_client::registry::{
    find_registry_agent, load_registry, refresh_registry, resolve_launch, RegistryFile,
    RegistrySource,
};
use aqbot_acp_client::runtime::{AcpEvent, AcpRuntime};
use aqbot_acp_client::types::AgentProbeResult;
use aqbot_core::repo::acp as acp_repo;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::{mpsc, Mutex};

/// Process-wide ACP runtime (permission channels + future process pool).
static ACP_RUNTIME: std::sync::OnceLock<Arc<AcpRuntime>> = std::sync::OnceLock::new();

fn runtime() -> Arc<AcpRuntime> {
    ACP_RUNTIME
        .get_or_init(|| Arc::new(AcpRuntime::new()))
        .clone()
}

// ---------- Registry & config ----------

#[tauri::command]
pub async fn acp_get_registry() -> Result<RegistryFile, String> {
    load_registry().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn acp_refresh_registry() -> Result<RegistryFile, String> {
    refresh_registry().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn acp_get_config() -> Result<AcpAgentsFile, String> {
    load_agents_file().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn acp_save_general(general: AcpGeneralConfig) -> Result<AcpAgentsFile, String> {
    let mut file = load_agents_file().map_err(|e| e.to_string())?;
    file.general = general;
    save_agents_file(&file).map_err(|e| e.to_string())?;
    Ok(file)
}

#[tauri::command]
pub async fn acp_add_agent_from_registry(
    agent_id: String,
    enabled: Option<bool>,
) -> Result<AcpAgentsFile, String> {
    let registry = load_registry().map_err(|e| e.to_string())?;
    let agent = find_registry_agent(&registry, &agent_id)
        .ok_or_else(|| format!("agent `{agent_id}` not in registry"))?;
    let mut file = load_agents_file().map_err(|e| e.to_string())?;
    upsert_from_registry(&mut file, agent, enabled.unwrap_or(true)).map_err(|e| e.to_string())?;
    save_agents_file(&file).map_err(|e| e.to_string())?;
    Ok(file)
}

#[tauri::command]
pub async fn acp_upsert_custom_agent(agent: ConfiguredAgent) -> Result<AcpAgentsFile, String> {
    let mut file = load_agents_file().map_err(|e| e.to_string())?;
    if let Some(existing) = file.agents.iter_mut().find(|a| a.id == agent.id) {
        *existing = agent;
    } else {
        file.agents.push(agent);
    }
    save_agents_file(&file).map_err(|e| e.to_string())?;
    Ok(file)
}

#[tauri::command]
pub async fn acp_set_agent_enabled(
    agent_id: String,
    enabled: bool,
) -> Result<AcpAgentsFile, String> {
    let mut file = load_agents_file().map_err(|e| e.to_string())?;
    if !set_agent_enabled(&mut file, &agent_id, enabled) {
        return Err(format!("agent `{agent_id}` not configured"));
    }
    save_agents_file(&file).map_err(|e| e.to_string())?;
    Ok(file)
}

#[tauri::command]
pub async fn acp_reorder_agents(agent_ids: Vec<String>) -> Result<AcpAgentsFile, String> {
    let mut file = load_agents_file().map_err(|e| e.to_string())?;
    reorder_agents(&mut file, &agent_ids);
    save_agents_file(&file).map_err(|e| e.to_string())?;
    Ok(file)
}

#[tauri::command]
pub async fn acp_remove_agent(agent_id: String) -> Result<AcpAgentsFile, String> {
    let mut file = load_agents_file().map_err(|e| e.to_string())?;
    if !remove_agent(&mut file, &agent_id) {
        return Err(format!("agent `{agent_id}` not configured"));
    }
    save_agents_file(&file).map_err(|e| e.to_string())?;
    Ok(file)
}

#[tauri::command]
pub async fn acp_list_enabled_agents() -> Result<Vec<ConfiguredAgent>, String> {
    let file = load_agents_file().map_err(|e| e.to_string())?;
    Ok(enabled_agents(&file).into_iter().cloned().collect())
}

#[tauri::command]
pub async fn acp_probe_agent(agent_id: String) -> Result<AgentProbeResult, String> {
    let file = load_agents_file().map_err(|e| e.to_string())?;
    let agent = file
        .agents
        .iter()
        .find(|a| a.id == agent_id)
        .ok_or_else(|| format!("agent `{agent_id}` not configured"))?;
    Ok(probe_agent(agent))
}

#[tauri::command]
pub async fn acp_probe_all() -> Result<Vec<AgentProbeResult>, String> {
    let file = load_agents_file().map_err(|e| e.to_string())?;
    Ok(file.agents.iter().map(probe_agent).collect())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedLaunchView {
    pub agent_id: String,
    pub command: String,
    pub args: Vec<String>,
    pub kind: String,
}

#[tauri::command]
pub async fn acp_resolve_launch(agent_id: String) -> Result<Option<ResolvedLaunchView>, String> {
    let registry = load_registry().map_err(|e| e.to_string())?;
    let Some(agent) = find_registry_agent(&registry, &agent_id) else {
        return Ok(None);
    };
    Ok(resolve_launch(agent).map(|l| ResolvedLaunchView {
        agent_id,
        command: l.command,
        args: l.args,
        kind: l.kind,
    }))
}

// ---------- Projects / threads / messages ----------

#[tauri::command]
pub async fn acp_list_projects(
    state: State<'_, AppState>,
) -> Result<Vec<aqbot_core::entity::acp_projects::Model>, String> {
    acp_repo::list_projects(&state.sea_db)
        .await
        .map_err(|e| e.to_string())
}

/// Reorder projects like conversation categories (drag-and-drop sort).
#[tauri::command]
pub async fn acp_reorder_projects(
    state: State<'_, AppState>,
    project_ids: Vec<String>,
) -> Result<(), String> {
    acp_repo::reorder_projects(&state.sea_db, &project_ids)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn acp_list_all_threads(
    state: State<'_, AppState>,
) -> Result<Vec<aqbot_core::entity::acp_threads::Model>, String> {
    acp_repo::list_all_threads(&state.sea_db)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn acp_create_project(
    state: State<'_, AppState>,
    name: String,
    root_path: String,
) -> Result<aqbot_core::entity::acp_projects::Model, String> {
    let path = PathBuf::from(&root_path);
    if !path.is_dir() {
        return Err(format!("path is not a directory: {root_path}"));
    }
    acp_repo::create_project(&state.sea_db, &name, &root_path)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn acp_delete_project(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<(), String> {
    acp_repo::delete_project(&state.sea_db, &project_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn acp_update_project(
    state: State<'_, AppState>,
    project_id: String,
    name: Option<String>,
    root_path: Option<String>,
) -> Result<aqbot_core::entity::acp_projects::Model, String> {
    if let Some(ref path) = root_path {
        let pb = PathBuf::from(path);
        if !pb.is_dir() {
            return Err(format!("path is not a directory: {path}"));
        }
    }
    acp_repo::update_project(
        &state.sea_db,
        &project_id,
        name.as_deref(),
        root_path.as_deref(),
    )
    .await
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "project not found".to_string())
}

#[tauri::command]
pub async fn acp_list_threads(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Vec<aqbot_core::entity::acp_threads::Model>, String> {
    let _ = acp_repo::touch_project(&state.sea_db, &project_id).await;
    acp_repo::list_threads_for_project(&state.sea_db, &project_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn acp_create_thread(
    state: State<'_, AppState>,
    project_id: String,
    agent_id: String,
    title: Option<String>,
) -> Result<aqbot_core::entity::acp_threads::Model, String> {
    let file = load_agents_file().map_err(|e| e.to_string())?;
    if !file.agents.iter().any(|a| a.id == agent_id && a.enabled) {
        return Err(format!("agent `{agent_id}` is not enabled"));
    }
    let title = title
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| "New conversation".into());
    acp_repo::create_thread(&state.sea_db, &project_id, &agent_id, &title)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn acp_delete_thread(state: State<'_, AppState>, thread_id: String) -> Result<(), String> {
    // Drop live agent process for this thread (if any)
    runtime().drop_session(&thread_id).await;
    acp_repo::delete_thread(&state.sea_db, &thread_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn acp_list_messages(
    state: State<'_, AppState>,
    thread_id: String,
) -> Result<Vec<aqbot_core::entity::acp_messages::Model>, String> {
    acp_repo::list_messages(&state.sea_db, &thread_id)
        .await
        .map_err(|e| e.to_string())
}

// ---------- Prompt / permission ----------

#[tauri::command]
pub async fn acp_prompt(
    app: AppHandle,
    state: State<'_, AppState>,
    thread_id: String,
    prompt: String,
) -> Result<(), String> {
    let thread = acp_repo::get_thread(&state.sea_db, &thread_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "thread not found".to_string())?;

    let project = acp_repo::get_project(&state.sea_db, &thread.project_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "project not found".to_string())?;

    let file = load_agents_file().map_err(|e| e.to_string())?;
    let agent = file
        .agents
        .iter()
        .find(|a| a.id == thread.agent_id && a.enabled)
        .cloned()
        .ok_or_else(|| format!("agent `{}` not enabled", thread.agent_id))?;

    // Persist user message
    acp_repo::create_message(&state.sea_db, &thread_id, "user", &prompt, Some("done"), None)
        .await
        .map_err(|e| e.to_string())?;

    let assistant = acp_repo::create_message(
        &state.sea_db,
        &thread_id,
        "assistant",
        "",
        Some("streaming"),
        None,
    )
    .await
    .map_err(|e| e.to_string())?;

    acp_repo::update_thread_session(&state.sea_db, &thread_id, None, "running")
        .await
        .map_err(|e| e.to_string())?;

    // Align with chat agent permission modes:
    // - auto_approve / full_access: auto-approve all tool permission prompts
    // - accept_edits / prompt / default: require UI approval (PermissionCard)
    let auto_approve = matches!(
        file.general.permission_default.as_str(),
        "full_access" | "auto_approve"
    );
    let cwd = PathBuf::from(&project.root_path);
    let session_id = thread.acp_session_id.clone();
    let rt = runtime();
    let db = state.sea_db.clone();
    let assistant_id = assistant.id.clone();
    let thread_id_clone = thread_id.clone();

    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<AcpEvent>();
    let accumulated_text = Arc::new(Mutex::new(String::new()));
    let turn_started = std::time::Instant::now();

    // Forward events to frontend.
    // Tool calls are also injected as inline <tool-call> markers into the
    // assistant message text (same pattern as chat agent mode) so they appear
    // in chronological order inside the bubble — not dumped under the thread.
    let app_fwd = app.clone();
    let thread_for_events = thread_id.clone();
    let assistant_for_events = assistant_id.clone();
    let acc_for_events = accumulated_text.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(ev) = event_rx.recv().await {
            match &ev {
                AcpEvent::StreamText { text } => {
                    {
                        let mut acc = acc_for_events.lock().await;
                        acc.push_str(text);
                    }
                    let _ = app_fwd.emit(
                        "acp-stream-text",
                        serde_json::json!({
                            "threadId": thread_for_events,
                            "messageId": assistant_for_events,
                            "text": text,
                        }),
                    );
                }
                AcpEvent::StreamThinking { thinking } => {
                    let _ = app_fwd.emit(
                        "acp-stream-thinking",
                        serde_json::json!({
                            "threadId": thread_for_events,
                            "messageId": assistant_for_events,
                            "thinking": thinking,
                        }),
                    );
                }
                AcpEvent::PermissionRequest {
                    request_id,
                    raw,
                    options,
                } => {
                    let _ = app_fwd.emit(
                        "acp-permission-request",
                        serde_json::json!({
                            "threadId": thread_for_events,
                            "messageId": assistant_for_events,
                            "requestId": request_id,
                            "raw": raw,
                            "options": options,
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
                    // Chronological inline marker → stream + DB final text
                    let marker = build_acp_tool_call_marker(tool_call_id, title, kind, raw);
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
                        "acp-error",
                        serde_json::json!({
                            "threadId": thread_for_events,
                            "messageId": assistant_for_events,
                            "message": message,
                        }),
                    );
                }
                // Runtime may still emit Done; ignore for UI finalization —
                // the authoritative acp-done is emitted after DB persist below.
                AcpEvent::Done { .. } => {}
            }
        }
    });

    let acc_for_persist = accumulated_text.clone();
    tauri::async_runtime::spawn(async move {
        let result = rt
            .prompt(
                &thread_id_clone, // live process key = AQBot thread id
                &agent,
                cwd,
                prompt,
                session_id,
                auto_approve,
                event_tx,
            )
            .await;

        // Allow any in-flight stream chunks to land in the accumulator.
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        let final_text = acc_for_persist.lock().await.clone();
        let duration_ms = turn_started.elapsed().as_millis() as u64;
        let meta = serde_json::json!({ "duration_ms": duration_ms }).to_string();

        match result {
            Ok(outcome) => {
                let _ = acp_repo::update_thread_session(
                    &db,
                    &thread_id_clone,
                    Some(&outcome.session_id),
                    "idle",
                )
                .await;
                let _ = acp_repo::update_message_content(
                    &db,
                    &assistant_id,
                    &final_text,
                    Some("done"),
                    Some(&meta),
                )
                .await;
                // Emit AFTER DB write so any subsequent loadMessages sees status=done.
                let _ = app.emit(
                    "acp-done",
                    serde_json::json!({
                        "threadId": thread_id_clone,
                        "messageId": assistant_id,
                        "stopReason": outcome.stop_reason,
                        "sessionId": outcome.session_id,
                        "text": final_text,
                        "durationMs": duration_ms,
                    }),
                );
            }
            Err(e) => {
                let err_text = if final_text.is_empty() {
                    format!("Error: {e}")
                } else {
                    format!("{final_text}\n\nError: {e}")
                };
                let _ = acp_repo::update_thread_session(&db, &thread_id_clone, None, "error").await;
                let _ = acp_repo::update_message_content(
                    &db,
                    &assistant_id,
                    &err_text,
                    Some("error"),
                    Some(&meta),
                )
                .await;
                let _ = app.emit(
                    "acp-error",
                    serde_json::json!({
                        "threadId": thread_id_clone,
                        "messageId": assistant_id,
                        "message": e.to_string(),
                        "text": err_text,
                        "durationMs": duration_ms,
                    }),
                );
            }
        }
    });

    Ok(())
}

#[tauri::command]
pub async fn acp_respond_permission(
    request_id: String,
    option_id: String,
) -> Result<(), String> {
    if runtime()
        .resolve_permission(&request_id, option_id)
        .await
    {
        Ok(())
    } else {
        Err("permission request not found or already resolved".into())
    }
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

// ---------- Inline tool-call markers (chat-agent parity) ----------

fn xml_attr_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn xml_text_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Build an inline `<tool-call>` marker so tools render mid-conversation
/// in call order (same contract as chat agent mode).
fn build_acp_tool_call_marker(
    tool_call_id: &str,
    title: &Option<String>,
    kind: &Option<String>,
    raw: &serde_json::Value,
) -> String {
    // Prefer short kind as the chip name; fall back to title / "tool"
    let name = kind
        .as_deref()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            title
                .as_deref()
                .map(|t| t.split_whitespace().next().unwrap_or(t))
                .filter(|s| !s.is_empty() && s.len() <= 32)
        })
        .unwrap_or("tool");

    let mut summary = title.clone().unwrap_or_default();
    if summary.is_empty() {
        // rawInput.command / path / filePath etc.
        let input = raw
            .get("rawInput")
            .or_else(|| raw.get("raw_input"))
            .or_else(|| raw.get("input"))
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        if let Some(obj) = input.as_object() {
            for key in ["command", "path", "filePath", "file_path", "pattern", "query"] {
                if let Some(v) = obj.get(key).and_then(|x| x.as_str()) {
                    summary = v.to_string();
                    break;
                }
            }
        }
        if summary.is_empty() {
            if let Some(locs) = raw.get("locations").and_then(|v| v.as_array()) {
                if let Some(path) = locs
                    .first()
                    .and_then(|l| l.get("path").or_else(|| l.get("uri")))
                    .and_then(|v| v.as_str())
                {
                    summary = path.to_string();
                }
            }
        }
    }

    // Keep summary readable in the chip
    if summary.len() > 160 {
        summary = format!("{}…", &summary[..160]);
    }
    // Collapse newlines for attr-like chip text
    summary = summary.replace('\n', " ").replace('\r', " ");

    format!(
        "\n\n<tool-call data-aqbot=\"1\" id=\"{}\" name=\"{}\">{}</tool-call>\n\n",
        xml_attr_escape(tool_call_id),
        xml_attr_escape(name),
        xml_text_escape(&summary),
    )
}

// ---------- Git (project working tree) ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpGitInfo {
    pub branch: Option<String>,
    pub branches: Vec<String>,
    pub is_repo: bool,
}

fn git_output(cwd: &std::path::Path, args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("git failed: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("git {} failed", args.join(" "))
        } else {
            stderr
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[tauri::command]
pub async fn acp_git_info(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<AcpGitInfo, String> {
    let project = acp_repo::get_project(&state.sea_db, &project_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "project not found".to_string())?;
    let cwd = PathBuf::from(&project.root_path);

    // Not a git repo → soft empty result
    let is_repo = std::process::Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(&cwd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !is_repo {
        return Ok(AcpGitInfo {
            branch: None,
            branches: vec![],
            is_repo: false,
        });
    }

    let branch = git_output(&cwd, &["branch", "--show-current"]).ok();
    let branch = branch.filter(|b| !b.is_empty());

    // Local branches (no remote-only clutter)
    let raw = git_output(&cwd, &["branch", "--format=%(refname:short)"]).unwrap_or_default();
    let mut branches: Vec<String> = raw
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    branches.sort();
    branches.dedup();

    Ok(AcpGitInfo {
        branch,
        branches,
        is_repo: true,
    })
}

#[tauri::command]
pub async fn acp_git_checkout(
    state: State<'_, AppState>,
    project_id: String,
    branch: String,
) -> Result<AcpGitInfo, String> {
    let project = acp_repo::get_project(&state.sea_db, &project_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "project not found".to_string())?;
    let cwd = PathBuf::from(&project.root_path);
    let branch = branch.trim();
    if branch.is_empty() {
        return Err("branch name is empty".into());
    }
    git_output(&cwd, &["checkout", branch])?;
    acp_git_info(state, project_id).await
}
