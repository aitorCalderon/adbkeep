//! Finding the adb executable: Preferences path → bundled platform-tools → PATH.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum AdbSource {
    /// Chosen by the user in Preferences.
    Custom,
    /// platform-tools shipped with the app.
    Bundled,
    /// Found in PATH.
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdbLocation {
    pub path: PathBuf,
    pub source: AdbSource,
}

pub fn adb_file_name() -> &'static str {
    if cfg!(windows) { "adb.exe" } else { "adb" }
}

/// `custom` is the path from Preferences; `bundled_dir` the bundled platform-tools directory.
/// A custom path that does not exist is an error instead of silently falling back.
pub fn locate(custom: Option<&Path>, bundled_dir: Option<&Path>) -> Result<AdbLocation> {
    if let Some(path) = custom {
        if !path.is_file() {
            return Err(Error::AdbNotUsable {
                path: path.display().to_string(),
                reason: "not a file".into(),
            });
        }
        return Ok(AdbLocation { path: path.to_path_buf(), source: AdbSource::Custom });
    }
    if let Some(dir) = bundled_dir {
        let path = dir.join(adb_file_name());
        if path.is_file() {
            return Ok(AdbLocation { path, source: AdbSource::Bundled });
        }
    }
    which::which("adb")
        .map(|path| AdbLocation { path, source: AdbSource::System })
        .map_err(|_| Error::AdbNotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_custom_path_is_an_error() {
        let err = locate(Some(Path::new("/nonexistent/adb")), None).unwrap_err();
        assert!(matches!(err, Error::AdbNotUsable { .. }));
    }

    #[test]
    fn bundled_wins_over_path() {
        let dir = std::env::temp_dir().join(format!("adbkeep-locate-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(adb_file_name()), b"").unwrap();
        let loc = locate(None, Some(&dir)).unwrap();
        assert_eq!(loc.source, AdbSource::Bundled);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
