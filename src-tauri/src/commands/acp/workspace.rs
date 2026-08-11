// ACP projects, threads, recent workspaces, and persisted messages.

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpRecentThreadReceipt {
    project: aqbot_core::entity::acp_projects::Model,
    thread: aqbot_core::entity::acp_threads::Model,
}

fn allocate_recent_workspace_path(settings: &AppSettings) -> Result<(PathBuf, String), String> {
    let workspace_id = aqbot_core::utils::gen_id();
    let created_at = chrono::Utc::now().timestamp();
    let workspace_dir =
        super::agent::resolve_agent_workspace_dir_for(settings, &workspace_id, created_at);
    let root_path = workspace_dir
        .to_str()
        .ok_or_else(|| "invalid ACP workspace path encoding".to_string())?
        .to_string();
    Ok((workspace_dir, root_path))
}

async fn create_recent_workspace_project(
    state: &AppState,
    settings: &AppSettings,
    title: &str,
    draft: bool,
) -> Result<aqbot_core::entity::acp_projects::Model, String> {
    let (workspace_dir, root_path) = allocate_recent_workspace_path(settings)?;
    let project = if draft {
        acp_repo::create_recent_draft_workspace(&state.sea_db, title, &root_path).await
    } else {
        acp_repo::create_recent_workspace(&state.sea_db, title, &root_path).await
    }
    .map_err(|error| error.to_string())?;
    if let Err(error) = std::fs::create_dir_all(&workspace_dir) {
        let rollback = acp_repo::delete_project(&state.sea_db, &project.id).await;
        return Err(match rollback {
            Ok(()) => format!("failed to create ACP workspace: {error}"),
            Err(rollback) => {
                format!("failed to create ACP workspace: {error}; rollback failed: {rollback}")
            }
        });
    }
    Ok(project)
}

async fn reusable_recent_draft(
    db: &sea_orm::DatabaseConnection,
) -> Result<Option<aqbot_core::entity::acp_projects::Model>, String> {
    let occupied_projects = acp_repo::list_all_threads(db)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|thread| thread.project_id)
        .collect::<HashSet<_>>();
    Ok(acp_repo::list_projects(db)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|project| project.kind == "recent_draft" && !occupied_projects.contains(&project.id)))
}

#[cfg(test)]
mod recent_draft_tests {
    use super::*;

    #[tokio::test]
    async fn only_an_explicit_unoccupied_recent_draft_is_reusable() {
        let db = aqbot_core::db::create_test_pool().await.unwrap().conn;
        let residual = acp_repo::create_recent_workspace(&db, "Deleted conversation", "/tmp/old")
            .await
            .unwrap();
        let draft =
            acp_repo::create_recent_draft_workspace(&db, "New conversation", "/tmp/recent-draft")
                .await
                .unwrap();

        assert_eq!(
            reusable_recent_draft(&db).await.unwrap().unwrap().id,
            draft.id
        );
        acp_repo::create_thread(&db, &draft.id, "codex", "Claimed")
            .await
            .unwrap();

        assert!(reusable_recent_draft(&db).await.unwrap().is_none());
        assert_eq!(residual.kind, "recent");
    }
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

/// Reserve one hidden Recent workspace for the composer before its first prompt.
/// Recent projects are only listed in the sidebar after they own a thread, so
/// this gives ACP a real cwd/session without creating an empty conversation.
#[tauri::command]
pub async fn acp_ensure_recent_draft(
    state: State<'_, AppState>,
) -> Result<aqbot_core::entity::acp_projects::Model, String> {
    let _guard = ACP_RECENT_DRAFT_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .await;
    if let Some(project) = reusable_recent_draft(&state.sea_db).await? {
        std::fs::create_dir_all(&project.root_path)
            .map_err(|error| format!("failed to restore ACP draft workspace: {error}"))?;
        return Ok(project);
    }

    let mut settings = aqbot_core::repo::settings::get_settings(&state.sea_db)
        .await
        .map_err(|error| error.to_string())?;
    settings.agent_workspace_root =
        aqbot_core::path_vars::decode_path_opt(&settings.agent_workspace_root);
    create_recent_workspace_project(&state, &settings, "New conversation", true).await
}

#[tauri::command]
pub async fn acp_delete_project(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<(), String> {
    let runtime = runtime();
    delete_project_with_runtime(&state.sea_db, &runtime, &project_id).await
}

async fn delete_project_with_runtime(
    db: &sea_orm::DatabaseConnection,
    runtime: &AcpRuntime,
    project_id: &str,
) -> Result<(), String> {
    let thread_ids = acp_repo::list_threads_for_project(db, project_id)
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|thread| thread.id)
        .collect::<Vec<_>>();
    for thread_id in &thread_ids {
        runtime
            .close_session(thread_id)
            .await
            .map_err(|error| format!("failed to close ACP thread `{thread_id}`: {error}"))?;
    }
    acp_repo::delete_project(db, project_id)
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
    if !file
        .agents
        .iter()
        .any(|agent| agent.id == agent_id && is_agent_enabled(agent))
    {
        return Err(format!("agent `{agent_id}` is not enabled"));
    }
    let title = title
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| "New conversation".into());
    let project = acp_repo::get_project(&state.sea_db, &project_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "project not found".to_string())?;
    let runtime = runtime();
    let draft_key = draft_session_key(&project_id, &agent_id);
    let (thread, draft_metadata_persisted) = match project.kind.as_str() {
        "recent_draft" => {
            let _guard = ACP_RECENT_DRAFT_LOCK
                .get_or_init(|| Mutex::new(()))
                .lock()
                .await;
            let snapshot = runtime
                .session_snapshot(&draft_key)
                .await
                .map_err(|error| format!("failed to inspect ACP Recent draft: {error}"))?;
            let mode_id = snapshot.as_ref().and_then(persisted_mode_id);
            let thread = acp_repo::claim_recent_draft_thread(
                &state.sea_db,
                &project_id,
                &agent_id,
                &title,
                snapshot.as_ref().map(|value| value.session_id.as_str()),
                mode_id.as_deref(),
            )
            .await
            .map_err(|error| error.to_string())?;
            (thread, snapshot.is_some())
        }
        "project" => (
            acp_repo::create_thread(&state.sea_db, &project_id, &agent_id, &title)
                .await
                .map_err(|error| error.to_string())?,
            false,
        ),
        _ => {
            return Err(format!(
                "ACP project `{project_id}` cannot accept another thread"
            ));
        }
    };
    let adopted = runtime.adopt_session(&draft_key, &thread.id).await;
    if !adopted || draft_metadata_persisted {
        return Ok(thread);
    }

    let snapshot = runtime
        .session_snapshot(&thread.id)
        .await
        .map_err(|error| format!("failed to inspect adopted ACP draft: {error}"))?
        .ok_or_else(|| "adopted ACP draft disappeared before persistence".to_string())?;
    persist_live_thread_snapshot(&state.sea_db, &thread.id, &snapshot, None).await?;
    acp_repo::get_thread(&state.sea_db, &thread.id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "newly created ACP thread disappeared".to_string())
}

#[tauri::command]
pub async fn acp_create_recent_thread(
    state: State<'_, AppState>,
    agent_id: String,
    title: Option<String>,
) -> Result<AcpRecentThreadReceipt, String> {
    let file = load_agents_file().map_err(|e| e.to_string())?;
    if !file
        .agents
        .iter()
        .any(|agent| agent.id == agent_id && is_agent_enabled(agent))
    {
        return Err(format!("agent `{agent_id}` is not enabled"));
    }
    let _guard = ACP_RECENT_DRAFT_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .await;

    let mut settings = aqbot_core::repo::settings::get_settings(&state.sea_db)
        .await
        .map_err(|error| error.to_string())?;
    settings.agent_workspace_root =
        aqbot_core::path_vars::decode_path_opt(&settings.agent_workspace_root);

    let title = title
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "New conversation".into());
    let project = create_recent_workspace_project(&state, &settings, &title, false).await?;
    let workspace_dir = PathBuf::from(&project.root_path);

    match acp_repo::create_thread(&state.sea_db, &project.id, &agent_id, &title).await {
        Ok(thread) => Ok(AcpRecentThreadReceipt { project, thread }),
        Err(error) => {
            if let Err(rollback) = acp_repo::delete_project(&state.sea_db, &project.id).await {
                return Err(format!(
                    "{error}; failed to roll back ACP Recent project: {rollback}"
                ));
            }
            std::fs::remove_dir(&workspace_dir).map_err(|cleanup| {
                format!("{error}; failed to remove empty ACP workspace: {cleanup}")
            })?;
            Err(error.to_string())
        }
    }
}

#[tauri::command]
pub async fn acp_delete_thread(
    state: State<'_, AppState>,
    thread_id: String,
) -> Result<(), String> {
    let runtime = runtime();
    delete_thread_with_runtime(&state.sea_db, &runtime, &thread_id).await
}

async fn delete_thread_with_runtime(
    db: &sea_orm::DatabaseConnection,
    runtime: &AcpRuntime,
    thread_id: &str,
) -> Result<(), String> {
    let project = match acp_repo::get_thread(db, thread_id)
        .await
        .map_err(|error| error.to_string())?
    {
        Some(thread) => acp_repo::get_project(db, &thread.project_id)
            .await
            .map_err(|error| error.to_string())?,
        None => None,
    };
    runtime
        .close_session(thread_id)
        .await
        .map_err(|error| format!("failed to close ACP thread `{thread_id}`: {error}"))?;
    acp_repo::delete_thread(db, thread_id)
        .await
        .map_err(|e| e.to_string())?;
    if let Some(project) = project.filter(|project| project.kind == "recent") {
        let remaining = acp_repo::list_threads_for_project(db, &project.id)
            .await
            .map_err(|error| error.to_string())?;
        if remaining.is_empty() {
            acp_repo::delete_project(db, &project.id)
                .await
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod session_delete_tests {
    use super::*;

    #[tokio::test]
    async fn close_failure_preserves_thread_and_project_records_and_live_session() {
        const AGENT: &str = r#"
import json
import sys

def respond(request_id, result):
    print(json.dumps({"jsonrpc": "2.0", "id": request_id, "result": result}), flush=True)

for line in sys.stdin:
    message = json.loads(line)
    method = message.get("method")
    if method == "initialize":
        respond(message["id"], {
            "protocolVersion": 1,
            "agentCapabilities": {"sessionCapabilities": {"close": {}}}
        })
    elif method == "session/new":
        respond(message["id"], {"sessionId": "delete-failure-session"})
    elif method == "session/close":
        print(json.dumps({
            "jsonrpc": "2.0",
            "id": message["id"],
            "error": {"code": -32000, "message": "forced close rejection"}
        }), flush=True)
"#;
        let db = aqbot_core::db::create_test_pool().await.unwrap().conn;
        let project = acp_repo::create_project(&db, "Project", "/tmp/project")
            .await
            .unwrap();
        let thread = acp_repo::create_thread(&db, &project.id, "failing-close", "Thread")
            .await
            .unwrap();
        let agent = ConfiguredAgent {
            id: "failing-close".into(),
            name: "Failing close".into(),
            enabled: true,
            source: "custom".into(),
            command: "python3".into(),
            args: vec!["-u".into(), "-c".into(), AGENT.into()],
            env: HashMap::new(),
            icon: None,
            sort: 0,
        };
        let runtime = AcpRuntime::new();
        runtime
            .prepare(
                &thread.id,
                &agent,
                std::env::current_dir().expect("current directory"),
                None,
                false,
                RuntimeLimits::new(60, 1),
                mpsc::unbounded_channel().0,
            )
            .await
            .expect("prepare deletable thread");

        let error = delete_thread_with_runtime(&db, &runtime, &thread.id)
            .await
            .expect_err("close rejection must abort deletion");

        assert!(error.contains("forced close rejection"), "{error}");
        assert!(acp_repo::get_thread(&db, &thread.id)
            .await
            .unwrap()
            .is_some());
        assert!(runtime.has_live_session(&thread.id).await);

        let project_error = delete_project_with_runtime(&db, &runtime, &project.id)
            .await
            .expect_err("close rejection must abort project deletion");
        assert!(
            project_error.contains("forced close rejection"),
            "{project_error}"
        );
        assert!(acp_repo::get_project(&db, &project.id)
            .await
            .unwrap()
            .is_some());
        assert!(acp_repo::get_thread(&db, &thread.id)
            .await
            .unwrap()
            .is_some());
        assert!(runtime.has_live_session(&thread.id).await);
    }
}

#[tauri::command]
pub async fn acp_rename_thread(
    state: State<'_, AppState>,
    thread_id: String,
    title: String,
) -> Result<aqbot_core::entity::acp_threads::Model, String> {
    acp_repo::update_thread_title(&state.sea_db, &thread_id, &title)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "thread not found".to_string())
}

#[tauri::command]
pub async fn acp_toggle_thread_pin(
    state: State<'_, AppState>,
    thread_id: String,
) -> Result<aqbot_core::entity::acp_threads::Model, String> {
    acp_repo::toggle_thread_pin(&state.sea_db, &thread_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "thread not found".to_string())
}

#[tauri::command]
pub async fn acp_reorder_threads(
    state: State<'_, AppState>,
    project_id: String,
    thread_ids: Vec<String>,
) -> Result<(), String> {
    acp_repo::reorder_threads(&state.sea_db, &project_id, &thread_ids)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn acp_duplicate_thread(
    state: State<'_, AppState>,
    thread_id: String,
    title_suffix: Option<String>,
) -> Result<aqbot_core::entity::acp_threads::Model, String> {
    let suffix = title_suffix.unwrap_or_else(|| " (copy)".into());
    acp_repo::duplicate_thread(&state.sea_db, &thread_id, &suffix)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "thread not found".to_string())
}

#[tauri::command]
pub async fn acp_list_messages(
    state: State<'_, AppState>,
    thread_id: String,
) -> Result<Vec<acp_repo::AcpMessageView>, String> {
    if !runtime().has_live_session(&thread_id).await {
        acp_repo::interrupt_streaming_messages(
            &state.sea_db,
            &thread_id,
            "The previous Agent turn was interrupted",
        )
        .await
        .map_err(|error| error.to_string())?;
    }
    acp_repo::list_messages(&state.sea_db, &thread_id)
        .await
        .map_err(|e| e.to_string())
}
