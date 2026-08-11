// ACP registry and agent configuration commands.

// ---------- Registry & config ----------

#[tauri::command]
pub async fn acp_get_registry() -> Result<RegistryFile, String> {
    load_registry().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn acp_refresh_registry(state: State<'_, AppState>) -> Result<RegistryFile, String> {
    let proxy_settings = load_process_proxy_settings(&state).await?;
    let proxy = resolve_proxy_environment(&proxy_settings).map_err(|error| error.to_string())?;
    let registry = refresh_registry_with_proxy(&proxy)
        .await
        .map_err(|e| e.to_string())?;
    let _guard = config_lock().lock().await;
    let mut file = load_agents_file().map_err(|e| e.to_string())?;
    let previous_agents = file.agents.clone();
    let synced = sync_configured_registry_agents(&mut file, &registry)
        .map_err(|error| format!("failed to apply refreshed ACP Registry: {error}"))?;
    let changed_agents = file
        .agents
        .iter()
        .filter(|agent| agent.source == "registry")
        .filter(|agent| {
            previous_agents
                .iter()
                .find(|previous| previous.id == agent.id)
                .is_some_and(|previous| {
                    previous.enabled != agent.enabled
                        || previous.command != agent.command
                        || previous.args != agent.args
                        || previous.env != agent.env
                })
        })
        .map(|agent| agent.id.clone())
        .collect::<Vec<_>>();
    if synced > 0 {
        save_agents_file(&file).map_err(|e| e.to_string())?;
    }
    if !changed_agents.is_empty() {
        note_launch_config_changed();
    }
    runtime().drop_agent_sessions(&changed_agents).await;
    Ok(registry)
}

#[tauri::command]
pub async fn acp_get_config() -> Result<AcpAgentsFile, String> {
    let _guard = config_lock().lock().await;
    migrate_agents_file().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn acp_save_general(general: AcpGeneralConfig) -> Result<AcpAgentsFile, String> {
    let _guard = config_lock().lock().await;
    let mut file = load_agents_file().map_err(|e| e.to_string())?;
    let launch_changed = general_launch_changed(&file.general, &general);
    let agent_ids = launch_changed.then(|| {
        file.agents
            .iter()
            .map(|agent| agent.id.clone())
            .collect::<Vec<_>>()
    });
    file.general = general;
    save_agents_file(&file).map_err(|e| e.to_string())?;
    if let Some(agent_ids) = agent_ids {
        note_launch_config_changed();
        runtime().drop_agent_sessions(&agent_ids).await;
    }
    Ok(file)
}

#[tauri::command]
pub async fn acp_add_agent_from_registry(
    agent_id: String,
    enabled: Option<bool>,
) -> Result<AcpAgentsFile, String> {
    let _guard = config_lock().lock().await;
    let registry = load_registry().map_err(|e| e.to_string())?;
    let agent = find_registry_agent(&registry, &agent_id)
        .ok_or_else(|| format!("agent `{agent_id}` not in registry"))?;
    if let Some(reason) = agent.quarantine_reason.as_deref() {
        return Err(format!(
            "agent `{agent_id}` is quarantined by the official ACP Registry: {reason}"
        ));
    }
    let mut file = load_agents_file().map_err(|e| e.to_string())?;
    let previous = file
        .agents
        .iter()
        .find(|configured| configured.id == agent_id)
        .cloned();
    upsert_from_registry(&mut file, agent, enabled.unwrap_or(true)).map_err(|e| e.to_string())?;
    save_agents_file(&file).map_err(|e| e.to_string())?;
    let current = file.agents.iter().find(|agent| agent.id == agent_id);
    if note_agent_launch_change(previous.as_ref(), current) {
        runtime()
            .drop_agent_sessions(std::slice::from_ref(&agent_id))
            .await;
    }
    Ok(file)
}

#[tauri::command]
pub async fn acp_upsert_custom_agent(agent: ConfiguredAgent) -> Result<AcpAgentsFile, String> {
    let _guard = config_lock().lock().await;
    let mut file = load_agents_file().map_err(|e| e.to_string())?;
    let agent_id = agent.id.clone();
    let previous = file
        .agents
        .iter()
        .find(|configured| configured.id == agent_id)
        .cloned();
    if let Some(existing) = file.agents.iter_mut().find(|a| a.id == agent.id) {
        *existing = agent;
    } else {
        file.agents.push(agent);
    }
    save_agents_file(&file).map_err(|e| e.to_string())?;
    let current = file.agents.iter().find(|agent| agent.id == agent_id);
    if note_agent_launch_change(previous.as_ref(), current) {
        runtime()
            .drop_agent_sessions(std::slice::from_ref(&agent_id))
            .await;
    }
    Ok(file)
}

#[tauri::command]
pub async fn acp_set_agent_enabled(
    agent_id: String,
    enabled: bool,
) -> Result<AcpAgentsFile, String> {
    let _guard = config_lock().lock().await;
    let mut file = load_agents_file().map_err(|e| e.to_string())?;
    let previous = file
        .agents
        .iter()
        .find(|agent| agent.id == agent_id)
        .cloned();
    if enabled
        && file
            .agents
            .iter()
            .find(|agent| agent.id == agent_id)
            .is_some_and(|agent| {
                agent.source == "registry"
                    && aqbot_acp_client::registry::official_quarantine_reason(&agent.id).is_some()
            })
    {
        return Err(format!(
            "agent `{agent_id}` is quarantined by the official ACP Registry"
        ));
    }
    if !set_agent_enabled(&mut file, &agent_id, enabled) {
        return Err(format!("agent `{agent_id}` not configured"));
    }
    save_agents_file(&file).map_err(|e| e.to_string())?;
    let current = file.agents.iter().find(|agent| agent.id == agent_id);
    if note_agent_launch_change(previous.as_ref(), current) {
        runtime()
            .drop_agent_sessions(std::slice::from_ref(&agent_id))
            .await;
    }
    Ok(file)
}

#[tauri::command]
pub async fn acp_reorder_agents(agent_ids: Vec<String>) -> Result<AcpAgentsFile, String> {
    let _guard = config_lock().lock().await;
    let mut file = load_agents_file().map_err(|e| e.to_string())?;
    reorder_agents(&mut file, &agent_ids);
    save_agents_file(&file).map_err(|e| e.to_string())?;
    Ok(file)
}

#[tauri::command]
pub async fn acp_remove_agent(agent_id: String) -> Result<AcpAgentsFile, String> {
    let _guard = config_lock().lock().await;
    let mut file = load_agents_file().map_err(|e| e.to_string())?;
    let previous = file
        .agents
        .iter()
        .find(|agent| agent.id == agent_id)
        .cloned();
    if !remove_agent(&mut file, &agent_id) {
        return Err(format!("agent `{agent_id}` not configured"));
    }
    save_agents_file(&file).map_err(|e| e.to_string())?;
    if note_agent_launch_change(previous.as_ref(), None) {
        runtime()
            .drop_agent_sessions(std::slice::from_ref(&agent_id))
            .await;
    }
    Ok(file)
}

#[tauri::command]
pub async fn acp_list_enabled_agents() -> Result<Vec<ConfiguredAgent>, String> {
    let file = load_agents_file().map_err(|e| e.to_string())?;
    Ok(enabled_agents(&file).into_iter().cloned().collect())
}

#[tauri::command]
pub async fn acp_probe_agent(
    state: State<'_, AppState>,
    agent_id: String,
) -> Result<AgentProbeResult, String> {
    let file = load_agents_file().map_err(|e| e.to_string())?;
    let agent = file
        .agents
        .iter()
        .find(|a| a.id == agent_id)
        .cloned()
        .ok_or_else(|| format!("agent `{agent_id}` not configured"))?;
    let proxy = load_process_proxy_settings(&state).await?;
    let agent = agent_with_process_proxy(agent, &proxy)?;
    Ok(probe_agent(&agent))
}

#[tauri::command]
pub async fn acp_probe_all(state: State<'_, AppState>) -> Result<Vec<AgentProbeResult>, String> {
    let file = load_agents_file().map_err(|e| e.to_string())?;
    let proxy = load_process_proxy_settings(&state).await?;
    let agents = file
        .agents
        .into_iter()
        .map(|agent| agent_with_process_proxy(agent, &proxy))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(agents.iter().map(probe_agent).collect())
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
