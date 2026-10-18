use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use adbkeep_core::backup::JobEvent;
use adbkeep_core::compat::CompatOptions;
use adbkeep_core::device::Device;
use adbkeep_core::device::probe::DeviceInfo;
use adbkeep_core::library;
use adbkeep_core::restore::{self, RestorePreview, RestoreRequest, RestoreStep};
use adbkeep_core::Error;
use tauri::{AppHandle, Manager, State};
use tauri_specta::Event;

use crate::events::JobProgressEvent;
use crate::state::{AppState, JobGuard};

fn ready(state: &AppState) -> Result<(Device, DeviceInfo), Error> {
    let device = state.devices.ready_device()?;
    let info = state.devices.snapshot().info.ok_or(Error::DeviceUnavailable { serial: device.serial().to_string() })?;
    Ok((device, info))
}

/// The backup's apps with their compatibility on the connected phone.
#[tauri::command]
#[specta::specta]
pub async fn restore_preview(
    state: State<'_, AppState>,
    backup_path: String,
    options: CompatOptions,
    use_helper: bool,
) -> Result<RestorePreview, Error> {
    let (device, info) = ready(&state)?;
    restore::preview(&device, &info, &PathBuf::from(backup_path), options, use_helper).await
}

async fn plan_for(device: &Device, info: &DeviceInfo, request: &RestoreRequest) -> Result<(RestorePreview, Vec<RestoreStep>), Error> {
    let opts = CompatOptions { bypass_low_target_sdk: request.bypass_low_target_sdk };
    let preview = restore::preview(device, info, &PathBuf::from(&request.backup_path), opts, true).await?;
    let metas = preview.apps.iter().map(|a| (a.meta.package.clone(), a.meta.clone())).collect();
    let compat: Vec<_> = preview.apps.iter().map(|a| a.compat.clone()).collect();
    let steps = restore::plan(&preview.manifest, &metas, &compat, request, &restore::file_sizes(&preview.manifest));
    Ok((preview, steps))
}

/// The commands a restore would run, without running them (advanced "Sólo simular").
#[tauri::command]
#[specta::specta]
pub async fn simulate_restore(state: State<'_, AppState>, request: RestoreRequest) -> Result<Vec<String>, Error> {
    let (device, info) = ready(&state)?;
    let (preview, steps) = plan_for(&device, &info, &request).await?;
    let root = device.root();
    let wifi = restore::wifi_networks(&PathBuf::from(&request.backup_path), &preview.manifest);
    let other: Vec<String> = request
        .other
        .items()
        .into_iter()
        .filter(|i| preview.manifest.other.iter().any(|e| e.item == *i))
        .flat_map(|i| adbkeep_core::other::restore::describe(i, &request.other, &wifi))
        .collect();
    Ok(steps
        .iter()
        .flat_map(|s| {
            let is_data = matches!(s.kind, restore::RestoreStepKind::Data { .. });
            s.commands.iter().enumerate().map(move |(i, c)| match (is_data && i == 1, root) {
                (true, Some(m)) => format!("adb shell {}", adbkeep_core::root::shell_quote(&adbkeep_core::root::as_root(m, c))),
                _ if c.starts_with("adb ") || c.starts_with("zstd ") => c.clone(),
                _ => format!("adb shell {}", adbkeep_core::root::shell_quote(c)),
            })
        })
        .chain(other)
        .collect())
}

/// Starts restoring in the background; progress arrives as `JobProgressEvent`.
#[tauri::command]
#[specta::specta]
pub async fn start_restore(app: AppHandle, state: State<'_, AppState>, request: RestoreRequest, use_helper: bool) -> Result<(), Error> {
    let (device, info) = ready(&state)?;
    // Room on the phone, and on the PC to unpack a single-file backup into.
    let (preview, steps) = plan_for(&device, &info, &request).await?;
    if let Some(storage) = &info.storage {
        let needed = restore::phone_space_needed(&preview, &request);
        if needed > storage.free_bytes {
            return Err(Error::NotEnoughSpace { needed, available: storage.free_bytes });
        }
    }
    let backup = PathBuf::from(&request.backup_path);
    if library::is_packed(&backup) {
        let cache = app.path().app_cache_dir().map_err(|e| Error::Io { reason: e.to_string() })?;
        library::check_space(preview.manifest.files.iter().map(|f| f.size).sum(), &cache)?;
    }
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut job = state.job.lock().unwrap();
        if job.is_some() {
            return Err(Error::Busy);
        }
        *job = Some(cancel.clone());
    }
    let cache = app.path().app_cache_dir().map_err(|e| Error::Io { reason: e.to_string() });
    let helper_dex = crate::commands::backup::helper_dex(&app, use_helper);
    tauri::async_runtime::spawn(async move {
        let _guard = JobGuard(app.clone());
        let emit = |e| {
            let _ = JobProgressEvent(e).emit(&app);
        };
        let result = async {
            let backup = PathBuf::from(&request.backup_path);
            // Packed backups are unpacked to the cache first, and removed afterwards.
            let (dir, temp) = if library::is_packed(&backup) {
                emit(JobEvent::Packing);
                let dest = cache?.join("restore").join(backup.file_stem().unwrap_or_default());
                let _ = std::fs::remove_dir_all(&dest);
                let (b, d) = (backup.clone(), dest.clone());
                tauri::async_runtime::spawn_blocking(move || adbkeep_core::archive::unpack(&b, &d))
                    .await
                    .map_err(|e| Error::Io { reason: e.to_string() })??;
                (dest.clone(), Some(dest))
            } else {
                (backup.clone(), None)
            };
            let tmp = std::env::temp_dir().join(format!("adbkeep-restore-{}", std::process::id()));
            let gate = app.state::<AppState>().devices.clone();
            let extras = adbkeep_core::backup::Extras { helper_dex: helper_dex.as_deref(), tmp: &tmp, gate: Some(&gate) };
            let r = restore::run(&device, info.sdk, &dir, &preview.manifest, &steps, &request, &extras, &cancel, &emit).await;
            let _ = std::fs::remove_dir_all(&tmp);
            if let Some(t) = temp {
                let _ = std::fs::remove_dir_all(t);
            }
            r
        }
        .await;
        if let Err(error) = result {
            emit(JobEvent::Failed { error });
        }
    });
    Ok(())
}
