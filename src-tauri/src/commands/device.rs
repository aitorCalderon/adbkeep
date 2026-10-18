use adbkeep_core::device::manager::DeviceSnapshot;
use adbkeep_core::root::RootMethod;
use adbkeep_core::Error;
use tauri::State;

use crate::state::AppState;

#[tauri::command]
#[specta::specta]
pub fn device_snapshot(state: State<'_, AppState>) -> DeviceSnapshot {
    state.devices.snapshot()
}

/// Chooses which phone to use when several are connected.
#[tauri::command]
#[specta::specta]
pub fn select_device(state: State<'_, AppState>, serial: String) {
    state.devices.select(serial);
}

/// Probes the selected phone again (e.g. after granting root on the phone).
#[tauri::command]
#[specta::specta]
pub fn retry_device(state: State<'_, AppState>) {
    state.devices.reprobe();
}

/// Advanced mode: pick another available root method.
#[tauri::command]
#[specta::specta]
pub fn set_root_method(state: State<'_, AppState>, method: RootMethod) {
    state.devices.set_root_method(method);
}

/// Runs `adb root` on the selected phone (debuggable builds, LineageOS rooted debugging).
#[tauri::command]
#[specta::specta]
pub async fn enable_adb_root(state: State<'_, AppState>) -> Result<(), Error> {
    state.devices.enable_adb_root().await
}
