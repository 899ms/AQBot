use crate::AppState;
use aqbot_core::types::*;
use std::sync::atomic::Ordering;
use tauri::AppHandle;
use tauri::Manager;
use tauri::State;

fn proxy_settings_changed(before: &AppSettings, after: &AppSettings) -> bool {
    before.proxy_type != after.proxy_type
        || before.proxy_address != after.proxy_address
        || before.proxy_port != after.proxy_port
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<AppSettings, String> {
    let mut settings = aqbot_core::repo::settings::get_settings(&state.sea_db)
        .await
        .map_err(|e| e.to_string())?;
    settings.backup_dir = aqbot_core::path_vars::decode_path_opt(&settings.backup_dir);
    settings.gateway_ssl_cert_path =
        aqbot_core::path_vars::decode_path_opt(&settings.gateway_ssl_cert_path);
    settings.gateway_ssl_key_path =
        aqbot_core::path_vars::decode_path_opt(&settings.gateway_ssl_key_path);
    settings.agent_workspace_root =
        aqbot_core::path_vars::decode_path_opt(&settings.agent_workspace_root);
    Ok(settings)
}

#[tauri::command]
pub async fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    mut settings: AppSettings,
) -> Result<(), String> {
    settings.selection_toolbar.validate()?;
    let observed_settings = aqbot_core::repo::settings::get_settings(&state.sea_db)
        .await
        .map_err(|e| e.to_string())?;
    let acp_guard = if proxy_settings_changed(&observed_settings, &settings) {
        Some(crate::commands::acp::config_lock().lock().await)
    } else {
        None
    };
    let invalidated_agent_ids = if acp_guard.is_some() {
        let current_settings = aqbot_core::repo::settings::get_settings(&state.sea_db)
            .await
            .map_err(|e| e.to_string())?;
        proxy_settings_changed(&current_settings, &settings)
            .then(crate::commands::acp::configured_agent_ids)
            .transpose()?
    } else {
        None
    };
    settings.backup_dir = aqbot_core::path_vars::encode_path_opt(&settings.backup_dir);
    settings.gateway_ssl_cert_path =
        aqbot_core::path_vars::encode_path_opt(&settings.gateway_ssl_cert_path);
    settings.gateway_ssl_key_path =
        aqbot_core::path_vars::encode_path_opt(&settings.gateway_ssl_key_path);
    settings.agent_workspace_root =
        aqbot_core::path_vars::encode_path_opt(&settings.agent_workspace_root);
    aqbot_core::repo::settings::save_settings(&state.sea_db, &settings)
        .await
        .map_err(|e| e.to_string())?;
    if let Some(agent_ids) = invalidated_agent_ids {
        crate::commands::acp::note_launch_config_changed();
        crate::commands::acp::invalidate_idle_agent_sessions(&agent_ids).await;
    }
    drop(acp_guard);

    let app_state = app.state::<AppState>();
    app_state
        .close_to_tray
        .store(settings.minimize_to_tray, Ordering::Relaxed);
    app_state
        .release_webview_on_tray
        .store(settings.release_webview_on_tray, Ordering::Relaxed);
    app_state.selection_toolbar.reconcile(&app, &settings).await;

    crate::tray::sync_tray_language(&app, &settings.language).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::proxy_settings_changed;
    use aqbot_core::types::AppSettings;

    #[test]
    fn unrelated_settings_do_not_invalidate_acp_processes() {
        let before = AppSettings::default();
        let mut after = before.clone();
        after.language = "en-US".into();

        assert!(!proxy_settings_changed(&before, &after));
    }

    #[test]
    fn every_proxy_field_change_invalidates_acp_processes() {
        let base = AppSettings {
            proxy_type: Some("http".into()),
            proxy_address: Some("127.0.0.1".into()),
            proxy_port: Some(7890),
            ..AppSettings::default()
        };

        let mut changed_type = base.clone();
        changed_type.proxy_type = Some("system".into());
        assert!(proxy_settings_changed(&base, &changed_type));

        let mut changed_address = base.clone();
        changed_address.proxy_address = Some("10.0.0.2".into());
        assert!(proxy_settings_changed(&base, &changed_address));

        let mut changed_port = base.clone();
        changed_port.proxy_port = Some(1080);
        assert!(proxy_settings_changed(&base, &changed_port));
    }
}
