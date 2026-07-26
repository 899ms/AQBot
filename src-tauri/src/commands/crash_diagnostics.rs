use tauri::State;

use crate::crash_diagnostics::{CrashDiagnosticsState, PreviousCrashReport};

#[tauri::command]
pub fn get_previous_crash_report(
    state: State<'_, CrashDiagnosticsState>,
) -> Result<Option<PreviousCrashReport>, String> {
    state.previous()
}

#[tauri::command]
pub fn acknowledge_previous_crash_report(
    state: State<'_, CrashDiagnosticsState>,
    id: String,
) -> Result<(), String> {
    state.acknowledge(&id)
}
