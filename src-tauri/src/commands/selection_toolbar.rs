use tauri::{AppHandle, Emitter, State};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::{
    selection_toolbar::{
        PermissionSettingsOutcome, RuntimeSnapshot, RuntimeStatus, SurfaceSize, ToolRunEvent,
        SELECTION_TOOLBAR_WINDOW_LABEL,
    },
    AppState,
};

#[tauri::command]
pub async fn selection_toolbar_get_runtime_status(
    state: State<'_, AppState>,
) -> Result<RuntimeStatus, String> {
    Ok(state.selection_toolbar.status().await)
}

#[tauri::command]
pub async fn selection_toolbar_get_snapshot(
    state: State<'_, AppState>,
) -> Result<RuntimeSnapshot, String> {
    Ok(state.selection_toolbar.snapshot().await)
}

#[tauri::command]
pub async fn selection_toolbar_open_permission_settings(
    state: State<'_, AppState>,
) -> Result<PermissionSettingsOutcome, String> {
    state.selection_toolbar.open_permission_settings()
}

#[tauri::command]
pub async fn selection_toolbar_request_permission(
    state: State<'_, AppState>,
) -> Result<crate::selection_toolbar::PermissionState, String> {
    state.selection_toolbar.request_permission()
}

#[tauri::command]
pub async fn selection_toolbar_retry_monitoring(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<RuntimeStatus, String> {
    state.selection_toolbar.retry(&app).await
}

#[tauri::command]
pub async fn selection_toolbar_set_surface(
    app: AppHandle,
    state: State<'_, AppState>,
    surface: SurfaceSize,
) -> Result<(), String> {
    state.selection_toolbar.set_surface(&app, surface).await
}

#[tauri::command]
pub async fn selection_toolbar_frontend_ready(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.selection_toolbar.mark_frontend_ready(&app).await;
    Ok(())
}

#[tauri::command]
pub async fn selection_toolbar_execute_tool(
    app: AppHandle,
    state: State<'_, AppState>,
    selection_id: String,
    tool_id: String,
    options: Option<crate::selection_toolbar::ToolRunOptions>,
) -> Result<String, String> {
    state.selection_toolbar.lock_interaction();
    let result = crate::selection_toolbar::execute_ai_tool(
        &app,
        state.inner(),
        &selection_id,
        &tool_id,
        options.unwrap_or_default(),
    )
    .await;
    if result.is_err() {
        state.selection_toolbar.unlock_interaction();
    }
    result
}

/// Persist the translate panel's target language (`None` follows the app
/// language again). Saved through the full settings pipeline so validation
/// and toolbar reconciliation behave exactly like the settings page.
#[tauri::command]
pub async fn selection_toolbar_set_translate_target(
    state: State<'_, AppState>,
    language: Option<String>,
) -> Result<(), String> {
    let mut settings = aqbot_core::repo::settings::get_settings(&state.sea_db)
        .await
        .map_err(|error| error.to_string())?;
    settings.selection_toolbar.translate_target_language =
        language.filter(|value| !value.trim().is_empty());
    settings.selection_toolbar.validate()?;
    aqbot_core::repo::settings::save_settings(&state.sea_db, &settings)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn selection_toolbar_stop_generation(
    app: AppHandle,
    state: State<'_, AppState>,
    request_id: String,
) -> Result<(), String> {
    if !state.selection_toolbar.stop_run(&request_id).await {
        return Err("The selection toolbar request is no longer active".into());
    }
    state.selection_toolbar.unlock_interaction();
    let snapshot = state.selection_toolbar.snapshot().await;
    let run = snapshot
        .run
        .ok_or_else(|| "The selection toolbar request is no longer active".to_string())?;
    let _ = app.emit_to(
        SELECTION_TOOLBAR_WINDOW_LABEL,
        "selection-toolbar://run",
        ToolRunEvent::Stopped {
            request_id,
            selection_id: run.selection_id,
            // The executor task follows up with the think-tag-finalized output.
            output: None,
        },
    );
    Ok(())
}

#[tauri::command]
pub async fn selection_toolbar_copy_selection(
    app: AppHandle,
    state: State<'_, AppState>,
    selection_id: String,
) -> Result<(), String> {
    state.selection_toolbar.lock_interaction();
    let text = match state.selection_toolbar.selection_text(&selection_id).await {
        Some(text) => text,
        None => {
            state.selection_toolbar.unlock_interaction();
            return Err("The selected text is no longer active".to_string());
        }
    };
    app.clipboard().write_text(text).map_err(|error| {
        state.selection_toolbar.unlock_interaction();
        error.to_string()
    })
}

#[tauri::command]
pub async fn selection_toolbar_copy_result(
    app: AppHandle,
    state: State<'_, AppState>,
    request_id: String,
) -> Result<(), String> {
    let output = state
        .selection_toolbar
        .run_output(&request_id)
        .await
        .ok_or_else(|| "The selection toolbar result is no longer available".to_string())?;
    // Copy only the answer — reasoning blocks stay in the panel.
    let output = strip_think_content_for_copy(&output);
    app.clipboard()
        .write_text(output)
        .map_err(|error| error.to_string())
}

/// Strip closed `<think>` blocks and truncate an unterminated one (copying
/// while the model is still reasoning must not leak partial thinking).
fn strip_think_content_for_copy(output: &str) -> String {
    let stripped = crate::commands::conversations::strip_think_tags(output);
    if let Some(start) = stripped.find("<think") {
        let after_tag = &stripped[start + 6..];
        if after_tag.starts_with('>') || after_tag.starts_with(' ') {
            return stripped[..start].trim_end().to_string();
        }
    }
    stripped
}

#[cfg(test)]
mod tests {
    use super::strip_think_content_for_copy;

    #[test]
    fn copy_strips_closed_and_unterminated_think_blocks() {
        assert_eq!(
            strip_think_content_for_copy(
                "<think totalMs=\"12\">\nreasoning\n</think>\n\nanswer"
            ),
            "answer"
        );
        assert_eq!(
            strip_think_content_for_copy("partial answer\n\n<think data-aqbot=\"1\">\nstill thinking"),
            "partial answer"
        );
        assert_eq!(
            strip_think_content_for_copy("1 < thinky 2"),
            "1 < thinky 2"
        );
    }
}

#[tauri::command]
pub async fn selection_toolbar_close(
    app: AppHandle,
    state: State<'_, AppState>,
    reason: String,
) -> Result<(), String> {
    state.selection_toolbar.hide(&app, &reason).await
}
