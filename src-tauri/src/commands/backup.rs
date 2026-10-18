use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use adbkeep_core::analysis::{self, Analysis};
use adbkeep_core::backup::{self, BackupRequest};
use adbkeep_core::device::state::ConnectionState;
use adbkeep_core::Error;
use tauri::{AppHandle, Manager, State};
use tauri_specta::Event;

use crate::events::{AnalysisEvent, JobProgressEvent};
use crate::state::{AnalysisGuard, AppState, JobGuard};

/// Analyses the ready phone in the background; progress arrives as `AnalysisEvent`.
#[tauri::command]
#[specta::specta]
pub fn start_analysis(app: AppHandle, state: State<'_, AppState>, use_helper: bool) -> Result<(), Error> {
    let device = state.devices.ready_device()?;
    if state.analysing.swap(true, Ordering::SeqCst) {
        return Err(Error::Busy);
    }
    let snapshot = state.devices.snapshot();
    let sdk = snapshot.info.map(|i| i.sdk).unwrap_or(0);
    let tmp = app.path().app_cache_dir().map_err(|e| Error::Io { reason: e.to_string() })?.join("analysis");
    let helper = helper_dex(&app, use_helper);
    tauri::async_runtime::spawn(async move {
        let _guard = AnalysisGuard(app.clone());
        let serial = device.serial().to_string();
        let store = |a: &Analysis| {
            *app.state::<AppState>().analysis.lock().unwrap() = Some((serial.clone(), a.clone()));
            let _ = AnalysisEvent(a.clone()).emit(&app);
        };
        if let Err(error) = analysis::analyze(&device, sdk, &tmp, helper.as_deref(), store).await {
            let failed = Analysis {
                stage: analysis::AnalysisStage::Done,
                apps: vec![],
                accounts: vec![],
                other: None,
                errors: vec![analysis::StageError { stage: analysis::AnalysisStage::Packages, error }],
            };
            let _ = AnalysisEvent(failed).emit(&app);
        }
    });
    Ok(())
}

/// The last analysis, if it belongs to the selected phone.
#[tauri::command]
#[specta::specta]
pub fn analysis_snapshot(state: State<'_, AppState>) -> Option<Analysis> {
    let selected = state.devices.snapshot().selected;
    state.analysis.lock().unwrap().as_ref().filter(|(s, _)| Some(s) == selected.as_ref()).map(|(_, a)| a.clone())
}

/// The adb commands a backup would run, without running them (advanced "Sólo simular").
#[tauri::command]
#[specta::specta]
pub fn simulate_backup(state: State<'_, AppState>, request: BackupRequest) -> Result<Vec<String>, Error> {
    let device = state.devices.ready_device()?;
    let apps = current_apps(&state, device.serial())?;
    Ok(backup::simulate(&device, &backup::plan(&apps, &request)))
}

/// Starts a backup in the background; progress arrives as `JobProgressEvent`.
#[tauri::command]
#[specta::specta]
pub fn start_backup(app: AppHandle, state: State<'_, AppState>, request: BackupRequest, use_helper: bool) -> Result<(), Error> {
    let device = state.devices.ready_device()?;
    let snapshot = state.devices.snapshot();
    let (ConnectionState::Ready, Some(info)) = (snapshot.state, snapshot.info) else {
        return Err(Error::DeviceUnavailable { serial: device.serial().to_string() });
    };
    let analysis = {
        let guard = state.analysis.lock().unwrap();
        let (_, a) = guard.as_ref().filter(|(s, _)| s == device.serial()).ok_or(Error::DeviceUnavailable {
            serial: device.serial().to_string(),
        })?;
        a.clone()
    };
    let helper_dex = helper_dex(&app, use_helper);
    let tmp = app.path().app_cache_dir().map_err(|e| Error::Io { reason: e.to_string() })?.join("backup");
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut job = state.job.lock().unwrap();
        if job.is_some() {
            return Err(Error::Busy);
        }
        *job = Some(cancel.clone());
    }
    let gate = state.devices.clone();
    tauri::async_runtime::spawn(async move {
        let _guard = JobGuard(app.clone());
        let emit = |e| {
            let _ = JobProgressEvent(e).emit(&app);
        };
        let extras = backup::Extras { helper_dex: helper_dex.as_deref(), tmp: &tmp, gate: Some(&gate) };
        let result = backup::run(&device, &info, &analysis, &request, &extras, &cancel, emit).await;
        let _ = std::fs::remove_dir_all(&tmp);
        if let Err(error) = result {
            let _ = JobProgressEvent(backup::JobEvent::Failed { error }).emit(&app);
        }
    });
    Ok(())
}

/// The selection an incomplete backup was started with, to resume it from the Copia screen.
#[tauri::command]
#[specta::specta]
pub fn saved_backup_request(path: String) -> Result<BackupRequest, Error> {
    backup::saved_request(std::path::Path::new(&path))
}

#[tauri::command]
#[specta::specta]
pub fn cancel_job(state: State<'_, AppState>) {
    if let Some(flag) = state.job.lock().unwrap().as_ref() {
        flag.store(true, Ordering::Relaxed);
    }
}

pub(crate) fn current_apps(state: &AppState, serial: &str) -> Result<Vec<analysis::AppInfo>, Error> {
    let guard = state.analysis.lock().unwrap();
    guard
        .as_ref()
        .filter(|(s, _)| s == serial)
        .map(|(_, a)| a.apps.clone())
        .ok_or(Error::DeviceUnavailable { serial: serial.to_string() })
}

/// The bundled helper dex, when enabled in Preferences. It is copied to /data/local/tmp on the
/// phone while working.
pub(crate) fn helper_dex(app: &AppHandle, use_helper: bool) -> Option<std::path::PathBuf> {
    use_helper
        .then(|| app.path().resource_dir().ok().map(|d| d.join("helper").join(adbkeep_core::helper::DEX_NAME)))
        .flatten()
        .filter(|p| p.is_file())
}
