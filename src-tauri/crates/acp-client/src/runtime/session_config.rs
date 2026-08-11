fn aqbot_client_capabilities() -> ClientCapabilities {
    ClientCapabilities::new()
        .session(ClientSessionCapabilities::new().config_options(
            SessionConfigOptionsCapabilities::new().boolean(BooleanConfigOptionCapabilities::new()),
        ))
        .elicitation(ElicitationCapabilities::new().form(ElicitationFormCapabilities::new()))
}

fn aqbot_initialize_request() -> InitializeRequest {
    InitializeRequest::new(ProtocolVersion::V1)
        .client_capabilities(aqbot_client_capabilities())
        .client_info(Implementation::new("aqbot", env!("CARGO_PKG_VERSION")).title("AQBot"))
}

async fn live_connection(live: &LiveSession) -> anyhow::Result<ConnectionTo<Agent>> {
    live.connection
        .lock()
        .await
        .clone()
        .ok_or_else(|| anyhow::anyhow!("ACP connection is not ready"))
}

async fn live_metadata(live: &LiveSession) -> anyhow::Result<AgentMetadata> {
    live.metadata
        .lock()
        .await
        .clone()
        .ok_or_else(|| anyhow::anyhow!("ACP agent metadata is not ready"))
}

fn snapshot_from_state(active: &ActiveSession, metadata: &AgentMetadata) -> AcpSessionSnapshot {
    AcpSessionSnapshot {
        session_id: active
            .id
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default(),
        modes: active.modes.clone(),
        config_options: active.config_options.clone(),
        agent_capabilities: metadata.capabilities.clone(),
    }
}

fn update_select_value(options: &mut [SessionConfigOption], config_id: &str, value: &str) {
    if let Some(option) = options
        .iter_mut()
        .find(|option| option.id.to_string() == config_id)
    {
        if let SessionConfigKind::Select(select) = &mut option.kind {
            select.current_value = value.to_string().into();
        }
    }
}

fn current_select_value(option: &SessionConfigOption) -> Option<String> {
    let SessionConfigKind::Select(select) = &option.kind else {
        return None;
    };
    Some(select.current_value.to_string())
}

fn current_config_value(option: &SessionConfigOption) -> Option<serde_json::Value> {
    match &option.kind {
        SessionConfigKind::Select(select) => {
            Some(serde_json::Value::String(select.current_value.to_string()))
        }
        SessionConfigKind::Boolean(boolean) => Some(serde_json::Value::Bool(boolean.current_value)),
        _ => None,
    }
}

fn restorable_config_selections(
    options: &[SessionConfigOption],
    replaced_config_id: &str,
) -> Vec<(String, serde_json::Value)> {
    options
        .iter()
        .filter(|option| option.id.to_string() != replaced_config_id)
        .filter(|option| !config_option_contains_plan(option))
        .filter(|option| {
            !option
                .meta
                .as_ref()
                .is_some_and(|meta| meta.contains_key("aqbotSpawnArg"))
        })
        .filter_map(|option| {
            current_config_value(option).map(|value| (option.id.to_string(), value))
        })
        .collect()
}

/// Encode the Agent's current plan/mode selection for thread persistence.
/// Standard modes keep their wire id for backward compatibility; config-backed
/// modes carry both the config id and value so they can be restored reliably.
pub fn persisted_mode_id(snapshot: &AcpSessionSnapshot) -> Option<String> {
    if let Some(modes) = snapshot.modes.as_ref() {
        return Some(modes.current_mode_id.to_string());
    }
    let option = snapshot
        .config_options
        .iter()
        .find(|option| config_option_contains_plan(option))?;
    let saved = PersistedConfigMode {
        config_id: option.id.to_string(),
        value: current_select_value(option)?,
    };
    Some(format!(
        "{PERSISTED_CONFIG_MODE_PREFIX}{}",
        serde_json::to_string(&saved).expect("string-only persisted mode is serializable")
    ))
}

fn send_grok_permission_mode(connection: &ConnectionTo<Agent>, mode: &str) -> anyhow::Result<()> {
    let payload = match mode {
        "default" => serde_json::json!({
            "permission_mode": "ask",
            "yolo_mode": false,
            "auto_mode": false,
        }),
        "auto" => serde_json::json!({
            "permission_mode": "auto",
            "yolo_mode": false,
            "auto_mode": true,
        }),
        "bypassPermissions" => serde_json::json!({
            "permission_mode": "always-approve",
            "yolo_mode": true,
            "auto_mode": false,
        }),
        _ => anyhow::bail!("unsupported Grok permission mode `{mode}`"),
    };
    let params = serde_json::value::to_raw_value(&payload)
        .map(Arc::from)
        .map_err(|error| anyhow::anyhow!("failed to encode Grok permission update: {error}"))?;
    connection
        .send_notification(ClientNotification::ExtNotification(ExtNotification::new(
            GROK_PERMISSION_SET_METHOD,
            params,
        )))
        .map_err(|error| anyhow::anyhow!("failed to update Grok permission mode: {error}"))
}

fn config_option_contains_value(option: &SessionConfigOption, expected: &str) -> bool {
    let SessionConfigKind::Select(select) = &option.kind else {
        return false;
    };
    match &select.options {
        SessionConfigSelectOptions::Ungrouped(options) => options
            .iter()
            .any(|option| option.value.to_string() == expected),
        SessionConfigSelectOptions::Grouped(groups) => groups.iter().any(|group| {
            group
                .options
                .iter()
                .any(|option| option.value.to_string() == expected)
        }),
        _ => false,
    }
}

fn sync_mode_config_values(options: &mut [SessionConfigOption], mode_id: &str) {
    for option in options.iter_mut().filter(|option| {
        option.category == Some(SessionConfigOptionCategory::Mode)
            && config_option_contains_plan(option)
            && config_option_contains_value(option, mode_id)
    }) {
        if let SessionConfigKind::Select(select) = &mut option.kind {
            select.current_value = mode_id.to_string().into();
        }
    }
}

fn sync_session_mode_from_config(
    active: &mut ActiveSession,
    option: &SessionConfigOption,
    mode_id: &str,
) {
    if option.category != Some(SessionConfigOptionCategory::Mode)
        || !config_option_contains_plan(option)
    {
        return;
    }
    let Some(modes) = active.modes.as_mut() else {
        return;
    };
    if modes
        .available_modes
        .iter()
        .any(|mode| mode.id.to_string() == mode_id)
    {
        modes.current_mode_id = SessionModeId::new(mode_id);
    }
}

fn apply_legacy_session_selection(
    options: &mut [SessionConfigOption],
    meta: Option<&agent_client_protocol::schema::v1::Meta>,
) {
    let Some(advertised) = meta
        .and_then(|meta| meta.get("x.ai/sessionConfig"))
        .and_then(|config| config.get("options"))
        .and_then(|options| options.as_array())
    else {
        return;
    };
    for selected in advertised.iter().filter(|option| {
        option
            .get("selected")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    }) {
        let Some(value) = selected.get("id").and_then(|value| value.as_str()) else {
            continue;
        };
        let target_id = match selected.get("category").and_then(|value| value.as_str()) {
            Some("model") => "model",
            Some("mode") => "reasoning_effort",
            _ => continue,
        };
        let is_known = options
            .iter()
            .find(|option| option.id.to_string() == target_id)
            .is_some_and(|option| validate_config_value(option, &serde_json::json!(value)).is_ok());
        if is_known {
            update_select_value(options, target_id, value);
        }
    }
}

fn agent_with_spawn_argument(
    agent: &ConfiguredAgent,
    flag: &str,
    value: &str,
) -> anyhow::Result<ConfiguredAgent> {
    if !["--model", "--reasoning-effort"].contains(&flag) {
        anyhow::bail!("unsupported ACP spawn option `{flag}`");
    }
    let value = value.trim();
    let use_agent_default = value == "__agent_default";
    if (!use_agent_default && value.is_empty())
        || value.len() > 64
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_.".contains(character))
    {
        anyhow::bail!("invalid ACP launch option value `{value}`");
    }

    let mut args = Vec::with_capacity(agent.args.len() + 2);
    let mut index = 0;
    while index < agent.args.len() {
        if agent.args[index] == flag {
            if index + 1 >= agent.args.len() {
                anyhow::bail!("ACP agent `{}` has `{flag}` without a value", agent.id);
            }
            index += 2;
        } else if agent.args[index].starts_with(&format!("{flag}=")) {
            index += 1;
        } else {
            args.push(agent.args[index].clone());
            index += 1;
        }
    }
    if use_agent_default {
        let mut updated = agent.clone();
        updated.args = args;
        return Ok(updated);
    }
    let transport_index = args
        .iter()
        .position(|argument| argument == "--acp")
        .or_else(|| args.iter().rposition(|argument| argument == "stdio"))
        .ok_or_else(|| anyhow::anyhow!("ACP agent `{}` has no ACP transport argument", agent.id))?;
    args.insert(transport_index, flag.to_string());
    args.insert(transport_index + 1, value.to_string());

    let mut updated = agent.clone();
    updated.args = args;
    Ok(updated)
}

pub fn configured_agent_with_reasoning_effort(
    agent: &ConfiguredAgent,
    effort: &str,
) -> anyhow::Result<ConfiguredAgent> {
    agent_with_spawn_argument(agent, "--reasoning-effort", effort)
}

pub fn configured_agent_with_model(
    agent: &ConfiguredAgent,
    model: &str,
) -> anyhow::Result<ConfiguredAgent> {
    agent_with_spawn_argument(agent, "--model", model)
}

fn launch_argument_value(agent: &ConfiguredAgent, flag: &str) -> Option<String> {
    agent.args.iter().enumerate().find_map(|(index, argument)| {
        if argument == flag {
            return agent.args.get(index + 1).cloned();
        }
        argument
            .strip_prefix(&format!("{flag}="))
            .map(str::to_string)
    })
}

fn copilot_probe_args(agent: &ConfiguredAgent, suffix: &[&str]) -> Vec<String> {
    let mut result = Vec::with_capacity(agent.args.len() + suffix.len());
    let mut index = 0;
    while index < agent.args.len() {
        let argument = &agent.args[index];
        if ["--model", "--reasoning-effort", "--effort"].contains(&argument.as_str()) {
            index += 2;
            continue;
        }
        if argument.starts_with("--model=")
            || argument.starts_with("--reasoning-effort=")
            || argument.starts_with("--effort=")
            || argument == "--acp"
            || argument == "--stdio"
        {
            index += 1;
            continue;
        }
        result.push(argument.clone());
        index += 1;
    }
    result.extend(suffix.iter().map(|argument| argument.to_string()));
    result
}

fn parse_copilot_models(help: &str) -> Vec<String> {
    let mut in_model_section = false;
    let mut models = Vec::new();
    for line in help.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("`model`:") {
            in_model_section = true;
            continue;
        }
        if !in_model_section {
            continue;
        }
        if trimmed.starts_with('`') {
            break;
        }
        let Some(quoted) = trimmed.strip_prefix("- \"") else {
            continue;
        };
        let Some(model) = quoted.strip_suffix('"') else {
            continue;
        };
        if !model.is_empty() && !models.iter().any(|known| known == model) {
            models.push(model.to_string());
        }
    }
    models
}

fn parse_copilot_reasoning_efforts(help: &str) -> Vec<String> {
    let flattened = help.split_whitespace().collect::<Vec<_>>().join(" ");
    let Some(flag_index) = flattened.find("--reasoning-effort") else {
        return Vec::new();
    };
    let remainder = &flattened[flag_index..];
    let Some(choice_index) = remainder.find("(choices:") else {
        return Vec::new();
    };
    let choices = &remainder[choice_index + "(choices:".len()..];
    let Some(end) = choices.find(')') else {
        return Vec::new();
    };
    choices[..end]
        .split(',')
        .map(|choice| choice.trim().trim_matches('"'))
        .filter(|choice| !choice.is_empty())
        .map(str::to_string)
        .collect()
}

async fn run_capability_probe(agent: &ConfiguredAgent, suffix: &[&str]) -> anyhow::Result<String> {
    let mut command = tokio::process::Command::new(&agent.command);
    command
        .args(copilot_probe_args(agent, suffix))
        .envs(agent.env.clone())
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(30), command.output())
        .await
        .map_err(|_| anyhow::anyhow!("ACP capability probe timed out"))??;
    if !output.status.success() {
        anyhow::bail!(
            "ACP capability probe failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    String::from_utf8(output.stdout)
        .map_err(|error| anyhow::anyhow!("ACP capability probe returned invalid UTF-8: {error}"))
}

async fn copilot_launch_catalog(agent: &ConfiguredAgent) -> anyhow::Result<LaunchOptionCatalog> {
    let key = format!(
        "{}\0{}",
        agent.command,
        copilot_probe_args(agent, &[]).join("\0")
    );
    let cache = LAUNCH_OPTION_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(cached) = cache.lock().await.get(&key).cloned() {
        return Ok(cached);
    }

    let (config_help, command_help) = tokio::try_join!(
        run_capability_probe(agent, &["help", "config"]),
        run_capability_probe(agent, &["--help"]),
    )?;
    let catalog = LaunchOptionCatalog {
        models: parse_copilot_models(&config_help),
        reasoning_efforts: parse_copilot_reasoning_efforts(&command_help),
    };
    if catalog.models.is_empty() || catalog.reasoning_efforts.is_empty() {
        anyhow::bail!(
            "GitHub Copilot capability discovery returned no {}",
            if catalog.models.is_empty() {
                "models"
            } else {
                "reasoning levels"
            }
        );
    }
    cache.lock().await.insert(key, catalog.clone());
    Ok(catalog)
}

fn launch_select_option(
    id: &str,
    name: &str,
    current: String,
    category: SessionConfigOptionCategory,
    flag: &str,
    values: &[String],
) -> SessionConfigOption {
    let mut choices = vec![
        SessionConfigSelectOption::new("__agent_default", "Agent default")
            .description("Use the agent's own configured default"),
    ];
    choices.extend(values.iter().map(|value| {
        SessionConfigSelectOption::new(
            value.clone(),
            if value == "auto" {
                "Auto".to_string()
            } else {
                value.clone()
            },
        )
    }));
    if !choices
        .iter()
        .any(|choice| choice.value.to_string() == current)
    {
        choices.push(SessionConfigSelectOption::new(
            current.clone(),
            current.clone(),
        ));
    }
    let mut marker = serde_json::Map::new();
    marker.insert(
        "aqbotSpawnArg".into(),
        serde_json::Value::String(flag.to_string()),
    );
    marker.insert(
        "aqbotCapabilitySource".into(),
        serde_json::Value::String("registry-cli".into()),
    );
    SessionConfigOption::select(
        id.to_string(),
        name.to_string(),
        current,
        SessionConfigSelectOptions::Ungrouped(choices),
    )
    .category(category)
    .meta(marker)
}

fn launch_live_model_option(current: String, values: &[String]) -> SessionConfigOption {
    let current = if current == "__agent_default" {
        values
            .iter()
            .find(|value| value.as_str() == "auto")
            .or_else(|| values.first())
            .cloned()
            .unwrap_or(current)
    } else {
        current
    };
    let mut option = launch_select_option(
        "model",
        "Model",
        current,
        SessionConfigOptionCategory::Model,
        "--model",
        values,
    );
    if let SessionConfigKind::Select(select) = &mut option.kind {
        if let SessionConfigSelectOptions::Ungrouped(choices) = &mut select.options {
            choices.retain(|choice| choice.value.to_string() != "__agent_default");
        }
    }
    let meta = option.meta.get_or_insert_with(Default::default);
    meta.remove("aqbotSpawnArg");
    meta.insert(
        "aqbotSetMethod".into(),
        serde_json::Value::String("session/set_model".into()),
    );
    option
}

async fn discover_launch_config_options(
    agent: &ConfiguredAgent,
) -> anyhow::Result<Vec<SessionConfigOption>> {
    let executable = std::path::Path::new(&agent.command)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(&agent.command)
        .to_ascii_lowercase();
    let is_copilot_acp = agent
        .args
        .iter()
        .any(|argument| argument.contains("@github/copilot"))
        || (executable == "copilot" && agent.args.iter().any(|argument| argument == "--acp"));
    if !is_copilot_acp {
        return Ok(Vec::new());
    }
    let catalog = copilot_launch_catalog(agent).await?;
    let mut models = vec!["auto".to_string()];
    models.extend(catalog.models);
    models.dedup();
    Ok(vec![
        launch_live_model_option(
            launch_argument_value(agent, "--model").unwrap_or_else(|| "__agent_default".into()),
            &models,
        ),
        launch_select_option(
            "reasoning_effort",
            "Reasoning",
            launch_argument_value(agent, "--reasoning-effort")
                .or_else(|| launch_argument_value(agent, "--effort"))
                .unwrap_or_else(|| "__agent_default".into()),
            SessionConfigOptionCategory::ThoughtLevel,
            "--reasoning-effort",
            &catalog.reasoning_efforts,
        ),
    ])
}

fn validate_config_value(
    option: &SessionConfigOption,
    value: &serde_json::Value,
) -> anyhow::Result<()> {
    match &option.kind {
        SessionConfigKind::Boolean(_) if value.is_boolean() => Ok(()),
        SessionConfigKind::Boolean(_) => {
            anyhow::bail!("config option `{}` requires a boolean", option.id)
        }
        SessionConfigKind::Select(select) => {
            let selected = value.as_str().ok_or_else(|| {
                anyhow::anyhow!("config option `{}` requires a string", option.id)
            })?;
            let exists = match &select.options {
                SessionConfigSelectOptions::Ungrouped(options) => options
                    .iter()
                    .any(|option| option.value.to_string() == selected),
                SessionConfigSelectOptions::Grouped(groups) => groups.iter().any(|group| {
                    group
                        .options
                        .iter()
                        .any(|option| option.value.to_string() == selected)
                }),
                _ => false,
            };
            if !exists {
                anyhow::bail!(
                    "unknown value `{selected}` for config option `{}`",
                    option.id
                );
            }
            Ok(())
        }
        _ => anyhow::bail!("unsupported config option type for `{}`", option.id),
    }
}

fn normalized_config_options(
    mut options: Vec<SessionConfigOption>,
    metadata: &AgentMetadata,
) -> Vec<SessionConfigOption> {
    // `aqbot*` metadata is host-reserved routing state. Never trust an Agent
    // supplied option to opt itself into process replacement or custom wire
    // methods; host-generated controls below add their markers afterwards.
    for option in &mut options {
        if let Some(meta) = option.meta.as_mut() {
            meta.retain(|key, _| !key.starts_with("aqbot"));
        }
    }
    if is_grok_shell(metadata)
        && !options
            .iter()
            .any(|option| is_agent_permission_config(option))
    {
        options.push(grok_permission_option("default"));
    }
    let has_model = options
        .iter()
        .any(|option| option.category == Some(SessionConfigOptionCategory::Model));
    if !has_model {
        if let Some(model) = legacy_model_option(metadata.meta.as_ref()) {
            options.push(model);
        }
    }
    let has_thought_level = options.iter().any(|option| {
        option.category == Some(SessionConfigOptionCategory::ThoughtLevel)
            || option.id.to_string() == "reasoning_effort"
    });
    if !has_thought_level {
        if let Some(reasoning) = legacy_reasoning_option(metadata.meta.as_ref()) {
            options.push(reasoning);
        }
    }
    for launch_option in &metadata.launch_config_options {
        let already_advertised = options.iter().any(|option| {
            option.id == launch_option.id
                || (launch_option.category.is_some() && option.category == launch_option.category)
        });
        if !already_advertised {
            options.push(launch_option.clone());
        }
    }
    options
}

fn normalized_config_options_for_session(
    options: Vec<SessionConfigOption>,
    metadata: &AgentMetadata,
    previous: &[SessionConfigOption],
) -> Vec<SessionConfigOption> {
    let mut normalized = normalized_config_options(options, metadata);
    for launch_option in &metadata.launch_config_options {
        let id = launch_option.id.to_string();
        let Some(previous_value) = previous
            .iter()
            .find(|option| option.id.to_string() == id)
            .and_then(current_config_value)
        else {
            continue;
        };
        let Some(option) = normalized
            .iter_mut()
            .find(|option| option.id.to_string() == id)
        else {
            continue;
        };
        if validate_config_value(option, &previous_value).is_err() {
            continue;
        }
        match (&mut option.kind, previous_value) {
            (SessionConfigKind::Select(select), serde_json::Value::String(value)) => {
                select.current_value = value.into();
            }
            (SessionConfigKind::Boolean(boolean), serde_json::Value::Bool(value)) => {
                boolean.current_value = value;
            }
            _ => {}
        }
    }
    normalized
}

fn is_grok_shell(metadata: &AgentMetadata) -> bool {
    metadata
        .meta
        .as_ref()
        .and_then(|meta| meta.get("grokShell"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn grok_permission_option(current: &str) -> SessionConfigOption {
    let mut marker = agent_client_protocol::schema::v1::Meta::new();
    marker.insert(
        "aqbotSetMethod".into(),
        serde_json::Value::String(GROK_PERMISSION_SET_METHOD.into()),
    );
    SessionConfigOption::select(
        GROK_PERMISSION_CONFIG_ID,
        "Permissions",
        current.to_string(),
        vec![
            SessionConfigSelectOption::new("default", "Ask")
                .description("Ask before protected tool calls"),
            SessionConfigSelectOption::new("auto", "Auto")
                .description("Use Grok's permission classifier"),
            SessionConfigSelectOption::new("bypassPermissions", "Always Approve")
                .description("Approve protected tool calls automatically"),
        ],
    )
    .category(SessionConfigOptionCategory::Other("permissions".into()))
    .meta(marker)
}

fn normalized_session_modes(
    modes: Option<SessionModeState>,
    metadata: &AgentMetadata,
) -> Option<SessionModeState> {
    modes.or_else(|| {
        is_grok_shell(metadata).then(|| {
            SessionModeState::new(
                "default",
                vec![
                    SessionMode::new("default", "Agent")
                        .description("Use Grok's normal coding mode"),
                    SessionMode::new("plan", "Plan")
                        .description("Create and review a plan without editing files"),
                ],
            )
        })
    })
}

fn legacy_model_option(
    meta: Option<&agent_client_protocol::schema::v1::Meta>,
) -> Option<SessionConfigOption> {
    let model_state = meta?.get("modelState")?;
    legacy_model_option_from_state(model_state)
}

fn legacy_model_option_from_state(model_state: &serde_json::Value) -> Option<SessionConfigOption> {
    let model_state = model_state.as_object()?;
    let current = model_state.get("currentModelId")?.as_str()?;
    let available = model_state.get("availableModels")?.as_array()?;
    let choices = available
        .iter()
        .filter_map(|model| {
            let id = model.get("modelId")?.as_str()?;
            let name = model
                .get("name")
                .and_then(|value| value.as_str())
                .unwrap_or(id);
            Some(SessionConfigSelectOption::new(
                id.to_string(),
                name.to_string(),
            ))
        })
        .collect::<Vec<_>>();
    if choices.is_empty() {
        return None;
    }
    let mut marker = serde_json::Map::new();
    marker.insert(
        "aqbotSetMethod".into(),
        serde_json::Value::String("session/set_model".into()),
    );
    Some(
        SessionConfigOption::select(
            "model",
            "Model",
            current.to_string(),
            SessionConfigSelectOptions::Ungrouped(choices),
        )
        .category(SessionConfigOptionCategory::Model)
        .meta(marker),
    )
}

fn legacy_reasoning_option(
    meta: Option<&agent_client_protocol::schema::v1::Meta>,
) -> Option<SessionConfigOption> {
    let model_state = meta?.get("modelState")?;
    legacy_reasoning_option_from_state(model_state)
}

fn legacy_reasoning_option_from_state(
    model_state: &serde_json::Value,
) -> Option<SessionConfigOption> {
    let model_state = model_state.as_object()?;
    let current_model = model_state.get("currentModelId")?.as_str()?;
    legacy_reasoning_option_for_model_from_state(model_state, current_model)
}

fn legacy_reasoning_option_for_model_from_state(
    model_state: &serde_json::Map<String, serde_json::Value>,
    model_id: &str,
) -> Option<SessionConfigOption> {
    let model = model_state
        .get("availableModels")?
        .as_array()?
        .iter()
        .find(|model| model.get("modelId").and_then(|value| value.as_str()) == Some(model_id))?;
    let model_meta = model.get("_meta")?.as_object()?;
    let efforts = model_meta.get("reasoningEfforts")?.as_array()?;
    let current = model_meta
        .get("reasoningEffort")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .or_else(|| {
            efforts.iter().find_map(|effort| {
                effort
                    .get("default")
                    .and_then(|value| value.as_bool())
                    .filter(|is_default| *is_default)
                    .and_then(|_| {
                        effort
                            .get("value")
                            .or_else(|| effort.get("id"))
                            .and_then(|value| value.as_str())
                            .map(str::to_string)
                    })
            })
        })?;
    let choices = efforts
        .iter()
        .filter_map(|effort| {
            if let Some(value) = effort.as_str() {
                return Some(SessionConfigSelectOption::new(
                    value.to_string(),
                    value.to_string(),
                ));
            }
            let value = effort.get("value").or_else(|| effort.get("id"))?.as_str()?;
            let label = effort
                .get("label")
                .or_else(|| effort.get("name"))
                .and_then(|value| value.as_str())
                .unwrap_or(value);
            Some(
                SessionConfigSelectOption::new(value.to_string(), label.to_string()).description(
                    effort
                        .get("description")
                        .and_then(|value| value.as_str())
                        .map(str::to_string),
                ),
            )
        })
        .collect::<Vec<_>>();
    if choices.is_empty()
        || !choices
            .iter()
            .any(|choice| choice.value.to_string() == current)
    {
        return None;
    }
    let mut marker = serde_json::Map::new();
    marker.insert(
        "aqbotSetMethod".into(),
        serde_json::Value::String("session/set_model_reasoning".into()),
    );
    Some(
        SessionConfigOption::select(
            "reasoning_effort",
            "Reasoning",
            current,
            SessionConfigSelectOptions::Ungrouped(choices),
        )
        .category(SessionConfigOptionCategory::ThoughtLevel)
        .meta(marker),
    )
}

fn apply_legacy_model_selection(
    options: &mut Vec<SessionConfigOption>,
    meta: Option<&agent_client_protocol::schema::v1::Meta>,
    model_id: &str,
) {
    update_select_value(options, "model", model_id);
    let Some(model_state) = meta
        .and_then(|meta| meta.get("modelState"))
        .and_then(serde_json::Value::as_object)
    else {
        return;
    };
    let replacement = legacy_reasoning_option_for_model_from_state(model_state, model_id);
    let existing = options.iter().position(|option| {
        option
            .meta
            .as_ref()
            .and_then(|meta| meta.get("aqbotSetMethod"))
            .and_then(serde_json::Value::as_str)
            == Some("session/set_model_reasoning")
    });
    match (existing, replacement) {
        (Some(index), Some(replacement)) => options[index] = replacement,
        (Some(index), None) => {
            options.remove(index);
        }
        (None, Some(replacement)) => options.push(replacement),
        (None, None) => {}
    }
}
