//! "Mis copias". File work runs on blocking threads so the UI stays responsive.

use std::path::PathBuf;

use adbkeep_core::format::Manifest;
use adbkeep_core::library::{self, BackupListing, VerifyReport};
use adbkeep_core::viewer::{self, ArchiveEntry, Call, Contact, Conversation, PartData, SettingView, WifiView};
use adbkeep_core::Error;
use tauri::AppHandle;
use tauri_specta::Event;

use crate::events::VerifyProgressEvent;

/// Text files shown in the viewer are cut at this size.
const VIEW_LIMIT: usize = 2 * 1024 * 1024;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, Error> + Send + 'static) -> Result<T, Error> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| Error::Io { reason: e.to_string() })?
}

#[tauri::command]
#[specta::specta]
pub async fn list_backups(dir: String) -> Result<Vec<BackupListing>, Error> {
    blocking(move || library::list(&PathBuf::from(dir))).await
}

/// Gives the password of an encrypted pack for this session; returns its manifest.
#[tauri::command]
#[specta::specta]
pub async fn unlock_backup(path: String, password: String) -> Result<Manifest, Error> {
    blocking(move || adbkeep_core::archive::unlock(&PathBuf::from(path), &password)).await
}

#[tauri::command]
#[specta::specta]
pub async fn backup_manifest(path: String) -> Result<Manifest, Error> {
    blocking(move || library::read_manifest(&PathBuf::from(path))).await
}

/// A file of the backup as text (JSON, logs, XML; ABX converted to XML), cut at 2 MiB.
#[tauri::command]
#[specta::specta]
pub async fn read_backup_file(path: String, rel: String) -> Result<String, Error> {
    blocking(move || library::read_text(&PathBuf::from(path), &rel, VIEW_LIMIT)).await
}

/// Copies one file of the backup to `dest`.
#[tauri::command]
#[specta::specta]
pub async fn export_backup_file(path: String, rel: String, dest: String) -> Result<(), Error> {
    blocking(move || library::export_file(&PathBuf::from(path), &rel, &PathBuf::from(dest)).map(|_| ())).await
}

#[tauri::command]
#[specta::specta]
pub async fn backup_messages(path: String) -> Result<Vec<Conversation>, Error> {
    blocking(move || viewer::messages(&PathBuf::from(path))).await
}

#[tauri::command]
#[specta::specta]
pub async fn backup_mms_part(path: String, part: i64) -> Result<PartData, Error> {
    blocking(move || viewer::mms_part(&PathBuf::from(path), part)).await
}

#[tauri::command]
#[specta::specta]
pub async fn export_mms_part(path: String, part: i64, dest: String) -> Result<(), Error> {
    blocking(move || viewer::export_mms_part(&PathBuf::from(path), part, &PathBuf::from(dest))).await
}

/// Messages as *SMS Backup & Restore* XML; returns how many.
#[tauri::command]
#[specta::specta]
pub async fn export_sms_xml(path: String, dest: String) -> Result<u32, Error> {
    blocking(move || viewer::export_sms_xml(&PathBuf::from(path), &PathBuf::from(dest))).await
}

#[tauri::command]
#[specta::specta]
pub async fn backup_contacts(path: String) -> Result<Vec<Contact>, Error> {
    blocking(move || viewer::contacts(&PathBuf::from(path))).await
}

/// Contacts as `.vcf`; returns how many cards.
#[tauri::command]
#[specta::specta]
pub async fn export_vcf(path: String, dest: String) -> Result<u32, Error> {
    blocking(move || viewer::export_vcf(&PathBuf::from(path), &PathBuf::from(dest))).await
}

#[tauri::command]
#[specta::specta]
pub async fn backup_calls(path: String) -> Result<Vec<Call>, Error> {
    blocking(move || viewer::calls(&PathBuf::from(path))).await
}

/// Saved networks; passwords only with `reveal`.
#[tauri::command]
#[specta::specta]
pub async fn backup_wifi(path: String, reveal: bool) -> Result<Vec<WifiView>, Error> {
    blocking(move || viewer::wifi(&PathBuf::from(path), reveal)).await
}

#[tauri::command]
#[specta::specta]
pub async fn backup_settings(path: String) -> Result<Vec<SettingView>, Error> {
    blocking(move || viewer::settings(&PathBuf::from(path))).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_archive(path: String, rel: String) -> Result<Vec<ArchiveEntry>, Error> {
    blocking(move || viewer::list_archive(&PathBuf::from(path), &rel)).await
}

/// Copies chosen files/folders of an archive into `dest`; returns how many files.
#[tauri::command]
#[specta::specta]
pub async fn extract_from_archive(path: String, rel: String, chosen: Vec<String>, dest: String) -> Result<u32, Error> {
    blocking(move || viewer::extract(&PathBuf::from(path), &rel, &chosen, &PathBuf::from(dest))).await
}

#[tauri::command]
#[specta::specta]
pub async fn verify_backup(app: AppHandle, path: String) -> Result<VerifyReport, Error> {
    blocking(move || {
        let p = PathBuf::from(&path);
        library::verify(&p, |done, total| {
            let _ = VerifyProgressEvent { path: path.clone(), done, total }.emit(&app);
        })
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn delete_backup(path: String) -> Result<(), Error> {
    blocking(move || library::delete(&PathBuf::from(path))).await
}

/// Folder → `.adbkeep`; returns the new path.
#[tauri::command]
#[specta::specta]
pub async fn pack_backup(path: String, password: Option<String>) -> Result<String, Error> {
    blocking(move || library::pack(&PathBuf::from(path), password.as_deref()).map(|p| p.display().to_string())).await
}

/// `.adbkeep` → folder; returns the new path.
#[tauri::command]
#[specta::specta]
pub async fn unpack_backup(path: String) -> Result<String, Error> {
    blocking(move || library::unpack(&PathBuf::from(path)).map(|p| p.display().to_string())).await
}
