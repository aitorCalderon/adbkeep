use adbkeep_core::analysis::Analysis;
use adbkeep_core::backup::JobEvent;
use adbkeep_core::device::manager::DeviceSnapshot;
use adbkeep_core::log::LogEntry;
use serde::Serialize;

/// A command was run (advanced-mode log panel).
#[derive(Debug, Clone, Serialize, specta::Type, tauri_specta::Event)]
pub struct LogEvent(pub LogEntry);

/// The connected phones or the selected one's state changed (device bar, connection panel).
#[derive(Debug, Clone, Serialize, specta::Type, tauri_specta::Event)]
pub struct DeviceEvent(pub DeviceSnapshot);

/// Analysis progress: the apps and accounts found so far.
#[derive(Debug, Clone, Serialize, specta::Type, tauri_specta::Event)]
pub struct AnalysisEvent(pub Analysis);

/// Backup or restore progress.
#[derive(Debug, Clone, Serialize, specta::Type, tauri_specta::Event)]
pub struct JobProgressEvent(pub JobEvent);

/// Integrity check progress (files checked of total) for a backup in "Mis copias".
#[derive(Debug, Clone, Serialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct VerifyProgressEvent {
    pub path: String,
    pub done: u32,
    pub total: u32,
}
