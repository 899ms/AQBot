// Process-wide ACP runtime and launch configuration synchronization.

/// Process-wide ACP runtime (permission channels + future process pool).
static ACP_RUNTIME: std::sync::OnceLock<Arc<AcpRuntime>> = std::sync::OnceLock::new();
static ACP_CONFIG_LOCK: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();
static ACP_RECENT_DRAFT_LOCK: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();
static ACP_LAUNCH_CONFIG_GENERATION: AtomicU64 = AtomicU64::new(0);
fn runtime() -> Arc<AcpRuntime> {
    ACP_RUNTIME
        .get_or_init(|| Arc::new(AcpRuntime::new()))
        .clone()
}

pub(crate) fn config_lock() -> &'static Mutex<()> {
    ACP_CONFIG_LOCK.get_or_init(|| Mutex::new(()))
}

pub(crate) fn note_launch_config_changed() {
    ACP_LAUNCH_CONFIG_GENERATION.fetch_add(1, AtomicOrdering::SeqCst);
}

fn agent_launch_changed(before: Option<&ConfiguredAgent>, after: Option<&ConfiguredAgent>) -> bool {
    match (before, after) {
        (Some(before), Some(after)) => {
            before.enabled != after.enabled
                || before.command != after.command
                || before.args != after.args
                || before.env != after.env
        }
        (None, None) => false,
        _ => true,
    }
}

fn note_agent_launch_change(
    before: Option<&ConfiguredAgent>,
    after: Option<&ConfiguredAgent>,
) -> bool {
    let changed = agent_launch_changed(before, after);
    if changed {
        note_launch_config_changed();
    }
    changed
}

fn general_launch_changed(before: &AcpGeneralConfig, after: &AcpGeneralConfig) -> bool {
    before.idle_timeout_secs != after.idle_timeout_secs
        || before.max_concurrent_processes != after.max_concurrent_processes
        || before.permission_default != after.permission_default
}

fn process_proxy_settings(settings: &AppSettings) -> ProcessProxySettings {
    ProcessProxySettings {
        proxy_type: settings.proxy_type.clone(),
        address: settings.proxy_address.clone(),
        port: settings.proxy_port,
    }
}

async fn load_process_proxy_settings(state: &AppState) -> Result<ProcessProxySettings, String> {
    let settings = aqbot_core::repo::settings::get_settings(&state.sea_db)
        .await
        .map_err(|error| error.to_string())?;
    Ok(process_proxy_settings(&settings))
}

struct LockedLaunchConfig {
    file: AcpAgentsFile,
    proxy: ProcessProxySettings,
    launch_generation: u64,
    _guard: tokio::sync::MutexGuard<'static, ()>,
}

/// Take one authoritative Agent launch snapshot. Holding the returned guard
/// until a process/session/prompt is accepted prevents older Agent or proxy
/// settings from being committed after a configuration mutation.
async fn load_locked_launch_config(state: &AppState) -> Result<LockedLaunchConfig, String> {
    let guard = config_lock().lock().await;
    let file = load_agents_file().map_err(|error| error.to_string())?;
    let proxy = load_process_proxy_settings(state).await?;
    Ok(LockedLaunchConfig {
        file,
        proxy,
        launch_generation: ACP_LAUNCH_CONFIG_GENERATION.load(AtomicOrdering::SeqCst),
        _guard: guard,
    })
}

fn agent_with_process_proxy(
    agent: ConfiguredAgent,
    proxy: &ProcessProxySettings,
) -> Result<ConfiguredAgent, String> {
    let agent_id = agent.id.clone();
    configured_agent_with_proxy(agent, proxy, resolve_system_proxy)
        .map_err(|error| format!("failed to configure proxy for ACP agent `{agent_id}`: {error}"))
}

pub(crate) fn configured_agent_ids() -> Result<Vec<String>, String> {
    Ok(load_agents_file()
        .map_err(|error| error.to_string())?
        .agents
        .into_iter()
        .map(|agent| agent.id)
        .collect())
}

pub(crate) async fn invalidate_idle_agent_sessions(agent_ids: &[String]) {
    runtime().drop_agent_sessions(agent_ids).await;
}

#[cfg(test)]
mod proxy_settings_tests {
    use super::{
        config_lock, launch_config_generation_is_current, note_agent_launch_change,
        note_launch_config_changed, overlay_enabled_agents_or_cleanup, process_proxy_settings,
        run_after_config_unlock, ACP_LAUNCH_CONFIG_GENERATION,
    };
    use aqbot_acp_client::config::{AcpAgentsFile, ConfiguredAgent};
    use aqbot_acp_client::proxy::ProcessProxySettings;
    use aqbot_core::types::AppSettings;
    use std::collections::HashMap;
    use std::sync::atomic::Ordering as AtomicOrdering;
    use std::time::Duration;
    use tokio::sync::oneshot;

    #[test]
    fn app_settings_map_to_process_proxy_settings_without_losing_system_mode() {
        let settings = AppSettings {
            proxy_type: Some("system".into()),
            proxy_address: Some("127.0.0.1".into()),
            proxy_port: Some(7890),
            ..AppSettings::default()
        };

        let proxy = process_proxy_settings(&settings);

        assert_eq!(proxy.proxy_type.as_deref(), Some("system"));
        assert_eq!(proxy.address.as_deref(), Some("127.0.0.1"));
        assert_eq!(proxy.port, Some(7890));
    }

    #[tokio::test]
    async fn slow_prewarm_work_does_not_block_foreground_launch_config() {
        let guard = config_lock().lock().await;
        let (started_tx, started_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let slow_prewarm = tokio::spawn(run_after_config_unlock(guard, async move {
            let _ = started_tx.send(());
            let _ = release_rx.await;
        }));

        started_rx.await.expect("slow prewarm must start");
        let foreground_guard = tokio::time::timeout(Duration::from_secs(1), config_lock().lock())
            .await
            .expect("foreground prepare must not wait for slow prewarm");
        drop(foreground_guard);
        let _ = release_tx.send(());
        slow_prewarm.await.expect("slow prewarm task must finish");
    }

    #[tokio::test]
    async fn launch_generation_rejects_proxy_disable_and_upsert_races() {
        let guard = config_lock().lock().await;
        let generation = ACP_LAUNCH_CONFIG_GENERATION.load(AtomicOrdering::SeqCst);

        // App proxy save.
        note_launch_config_changed();

        let original = ConfiguredAgent {
            id: "test-agent".into(),
            name: "Test Agent".into(),
            enabled: true,
            source: "custom".into(),
            command: "agent-v1".into(),
            args: Vec::new(),
            env: HashMap::new(),
            icon: None,
            sort: 0,
        };
        let mut disabled = original.clone();
        disabled.enabled = false;
        assert!(note_agent_launch_change(Some(&original), Some(&disabled)));

        let mut upserted = original.clone();
        upserted.command = "agent-v2".into();
        assert!(note_agent_launch_change(Some(&original), Some(&upserted)));
        assert_eq!(
            ACP_LAUNCH_CONFIG_GENERATION.load(AtomicOrdering::SeqCst),
            generation + 3
        );
        drop(guard);

        assert!(!launch_config_generation_is_current(generation).await);
        assert!(launch_config_generation_is_current(generation + 3).await);
    }

    #[tokio::test]
    async fn invalid_latest_proxy_cleans_all_idle_agent_ids_before_erroring() {
        let guard = config_lock().lock().await;
        let enabled = ConfiguredAgent {
            id: "enabled-agent".into(),
            name: "Enabled".into(),
            enabled: true,
            source: "custom".into(),
            command: "enabled-agent".into(),
            args: Vec::new(),
            env: HashMap::new(),
            icon: None,
            sort: 0,
        };
        let mut disabled = enabled.clone();
        disabled.id = "disabled-agent".into();
        disabled.name = "Disabled".into();
        disabled.enabled = false;
        let file = AcpAgentsFile {
            agents: vec![enabled, disabled],
            ..AcpAgentsFile::default()
        };
        let proxy = ProcessProxySettings {
            proxy_type: Some("http".into()),
            address: None,
            port: Some(7890),
        };
        let cleaned_ids = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let captured = cleaned_ids.clone();

        let error = overlay_enabled_agents_or_cleanup(&file, &proxy, move |agent_ids| async move {
            *captured.lock().await = agent_ids;
        })
        .await
        .expect_err("invalid proxy must reject prewarm");
        drop(guard);

        assert!(error.contains("proxy address is required"), "{error}");
        assert_eq!(
            *cleaned_ids.lock().await,
            vec!["enabled-agent".to_string(), "disabled-agent".to_string()]
        );
    }
}
