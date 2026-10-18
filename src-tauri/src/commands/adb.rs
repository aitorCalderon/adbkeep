use std::path::PathBuf;

use adbkeep_core::adb::AdbSource;
use adbkeep_core::Error;
use serde::Serialize;
use tauri::State;

use crate::state::AppState;

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AdbStatus {
    pub path: Option<String>,
    pub source: Option<AdbSource>,
    /// First line of `adb version`; `None` when adb is not usable.
    pub version: Option<String>,
    pub error: Option<Error>,
}

impl AdbStatus {
    pub fn failed(path: Option<String>, source: Option<AdbSource>, error: Error) -> Self {
        Self { path, source, version: None, error: Some(error) }
    }
}

/// (Re)configures adb. `custom_path` is the path from Preferences, empty or `None` for automatic.
#[tauri::command]
#[specta::specta]
pub async fn set_adb_path(state: State<'_, AppState>, custom_path: Option<String>) -> Result<AdbStatus, ()> {
    let custom = custom_path.filter(|p| !p.trim().is_empty()).map(PathBuf::from);
    Ok(state.configure_adb(custom).await)
}

/// Status of the last adb configuration; `None` until the UI has called [`set_adb_path`].
#[tauri::command]
#[specta::specta]
pub async fn adb_status(state: State<'_, AppState>) -> Result<Option<AdbStatus>, ()> {
    Ok(state.adb_status().await)
}
