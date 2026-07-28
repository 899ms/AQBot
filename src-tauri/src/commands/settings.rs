use crate::AppState;
use aqbot_core::types::*;
use std::sync::atomic::Ordering;
use tauri::AppHandle;
use tauri::Manager;
use tauri::State;

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
