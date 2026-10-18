use adbkeep_core::log::LogEntry;
use tauri::State;

use crate::state::AppState;

#[tauri::command]
#[specta::specta]
pub fn log_entries(state: State<'_, AppState>) -> Vec<LogEntry> {
    state.log.entries()
}

#[tauri::command]
#[specta::specta]
pub fn clear_log(state: State<'_, AppState>) {
    state.log.clear();
}
