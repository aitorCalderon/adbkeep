use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use adbkeep_core::analysis::Analysis;

use adbkeep_core::adb::{self, Adb};
use adbkeep_core::device::manager::DeviceManager;
use adbkeep_core::log::CommandLog;
use adbkeep_core::Error;
use tokio::sync::RwLock;

use crate::commands::adb::AdbStatus;

/// Frees the running-job slot when dropped, also when the task panics (otherwise every later
/// job would be refused as busy). A panic is reported to the UI as a failed job.
pub struct JobGuard(pub tauri::AppHandle);

impl Drop for JobGuard {
    fn drop(&mut self) {
        use tauri::Manager;
        use tauri_specta::Event;
        if std::thread::panicking() {
            let error = Error::Io { reason: "internal error".into() };
            let _ = crate::events::JobProgressEvent(adbkeep_core::backup::JobEvent::Failed { error }).emit(&self.0);
        }
        *self.0.state::<AppState>().job.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// Same for the analysis flag.
pub struct AnalysisGuard(pub tauri::AppHandle);

impl Drop for AnalysisGuard {
    fn drop(&mut self) {
        use tauri::Manager;
        use tauri_specta::Event;
        if std::thread::panicking() {
            let failed = Analysis {
                stage: adbkeep_core::analysis::AnalysisStage::Done,
                apps: vec![],
                accounts: vec![],
                other: None,
                errors: vec![adbkeep_core::analysis::StageError {
                    stage: adbkeep_core::analysis::AnalysisStage::Packages,
                    error: Error::Io { reason: "internal error".into() },
                }],
            };
            let _ = crate::events::AnalysisEvent(failed).emit(&self.0);
        }
        self.0.state::<AppState>().analysing.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

pub struct AppState {
    pub log: Arc<CommandLog>,
    pub devices: DeviceManager,
    bundled_adb_dir: Option<PathBuf>,
    adb: RwLock<Option<Adb>>,
    adb_status: RwLock<Option<AdbStatus>>,
    /// Last analysis and the phone it belongs to.
    pub analysis: std::sync::Mutex<Option<(String, Analysis)>>,
    /// An analysis is running (a second one would race with it on the phone).
    pub analysing: AtomicBool,
    /// Cancel flag of the running job; `None` when idle.
    pub job: std::sync::Mutex<Option<Arc<AtomicBool>>>,
}

impl AppState {
    pub fn new(log: Arc<CommandLog>, devices: DeviceManager, bundled_adb_dir: Option<PathBuf>) -> Self {
        Self { log, devices, bundled_adb_dir, adb: RwLock::new(None), adb_status: RwLock::new(None), analysis: Default::default(), analysing: AtomicBool::new(false), job: Default::default() }
    }

    /// Locates adb (Preferences path first) and checks that it runs.
    pub async fn configure_adb(&self, custom: Option<PathBuf>) -> AdbStatus {
        let located = adb::locate(custom.as_deref(), self.bundled_adb_dir.as_deref());
        let (adb, status) = match located {
            Err(error) => (None, AdbStatus::failed(None, None, error)),
            Ok(loc) => {
                let candidate = Adb::new(&loc.path, self.log.clone());
                let shown = loc.path.display().to_string();
                let path = Some(shown.clone());
                match candidate.version().await {
                    Ok(version) => (
                        Some(candidate),
                        AdbStatus { path, source: Some(loc.source), version: Some(version), error: None },
                    ),
                    Err(e) => (
                        None,
                        AdbStatus::failed(path, Some(loc.source), Error::AdbNotUsable { path: shown, reason: e.to_string() }),
                    ),
                }
            }
        };
        self.devices.set_adb(adb.clone());
        *self.adb.write().await = adb;
        *self.adb_status.write().await = Some(status.clone());
        status
    }

    /// The last [`Self::configure_adb`] result, or `None` before the first one.
    pub async fn adb_status(&self) -> Option<AdbStatus> {
        self.adb_status.read().await.clone()
    }

    #[allow(dead_code)] // used from phase 2 on
    pub async fn adb(&self) -> Result<Adb, Error> {
        self.adb.read().await.clone().ok_or(Error::AdbNotFound)
    }
}
