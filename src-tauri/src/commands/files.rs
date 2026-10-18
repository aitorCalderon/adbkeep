/// Writes text to a path the user picked in a save dialog (log, reports).
#[tauri::command]
#[specta::specta]
pub async fn save_text(path: String, contents: String) -> Result<(), adbkeep_core::Error> {
    tokio::fs::write(path, contents).await.map_err(Into::into)
}

/// Free bytes on the disk holding `path` (or its nearest existing parent).
#[tauri::command]
#[specta::specta]
pub fn free_space(path: String) -> Option<u64> {
    let mut p = std::path::PathBuf::from(path);
    while !p.exists() {
        p = p.parent()?.to_path_buf();
    }
    fs4::available_space(&p).ok()
}

/// `<Documents>/adbkeep`, used until a folder is chosen in Preferences.
#[tauri::command]
#[specta::specta]
pub fn default_backup_dir(app: tauri::AppHandle) -> Option<String> {
    use tauri::Manager;
    let docs = app.path().document_dir().or_else(|_| app.path().home_dir()).ok()?;
    Some(docs.join("adbkeep").display().to_string())
}
