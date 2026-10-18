//! On-disk backup format: one folder per backup with `manifest.json` (written last), APKs,
//! zstd-compressed tars and JSON metadata. See the plan's "Formato de la copia".

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::analysis::{Account, AppKind};
use crate::root::RootMethod;
use crate::{Error, Result};

/// Bumped on incompatible changes; readers refuse newer versions.
pub const FORMAT_VERSION: u32 = 1;
pub const MANIFEST: &str = "manifest.json";
/// Extension of a backup packed into one file.
pub const PACKED_EXT: &str = "adbkeep";

/// `path` with `.suffix` appended to the file name. Unlike `Path::with_extension`, dots already
/// in the name are kept ("v1.2" + "adbkeep" is "v1.2.adbkeep", not "v1.adbkeep").
pub fn with_suffix(path: &Path, suffix: &str) -> std::path::PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".");
    s.push(suffix);
    s.into()
}

/// `<folder>.adbkeep`, the packed form of a backup folder.
pub fn packed_path(dir: &Path) -> std::path::PathBuf {
    with_suffix(dir, PACKED_EXT)
}

/// Rejects names that are not a single, portable file name: empty, `.`/`..`, separators,
/// characters Windows forbids, a trailing dot or space, or a Windows reserved device name.
pub fn validate_name(name: &str) -> Result<&str> {
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.as_bytes()[3].is_ascii_digit());
    let bad = name.is_empty()
        || name == "."
        || name == ".."
        || name.ends_with(['.', ' '])
        || name.chars().any(|c| c.is_control() || "/\\:*?\"<>|".contains(c))
        || reserved;
    if bad { Err(Error::Io { reason: format!("{name:?}: invalid backup name") }) } else { Ok(name) }
}

/// Where an app's data lives (backup-android.md point 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum DataPart {
    Ce,
    De,
    ExtData,
    Obb,
    Media,
}

impl DataPart {
    pub const ALL: [DataPart; 5] = [DataPart::Ce, DataPart::De, DataPart::ExtData, DataPart::Obb, DataPart::Media];

    /// Parent directory on the phone (user 0); the package directory is inside it.
    pub fn base_dir(self) -> &'static str {
        match self {
            DataPart::Ce => "/data/data",
            DataPart::De => "/data/user_de/0",
            DataPart::ExtData => "/data/media/0/Android/data",
            DataPart::Obb => "/data/media/0/Android/obb",
            DataPart::Media => "/data/media/0/Android/media",
        }
    }

    pub fn archive_name(self) -> &'static str {
        match self {
            DataPart::Ce => "ce.tar.zst",
            DataPart::De => "de.tar.zst",
            DataPart::ExtData => "ext_data.tar.zst",
            DataPart::Obb => "obb.tar.zst",
            DataPart::Media => "media.tar.zst",
        }
    }

    /// Subdirectories left out: caches, the native-lib symlink and, unless asked, `no_backup`
    /// (FCM tokens, install IDs).
    pub fn excludes(self, include_no_backup: bool) -> Vec<&'static str> {
        match self {
            DataPart::Ce | DataPart::De => {
                let mut v = vec!["cache", "code_cache", "lib"];
                if !include_no_backup {
                    v.push("no_backup");
                }
                v
            }
            DataPart::ExtData => vec!["cache"],
            DataPart::Obb | DataPart::Media => vec![],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SourceDevice {
    pub serial: String,
    pub manufacturer: String,
    pub model: String,
    pub android_release: String,
    pub sdk: u32,
    pub abis: Vec<String>,
    pub build_type: String,
    pub build_id: String,
    pub root_method: Option<RootMethod>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ManifestApp {
    pub package: String,
    /// Name shown on the phone, when the helper provided it.
    #[serde(default)]
    pub label: Option<String>,
    pub kind: AppKind,
    pub version_code: u64,
    pub version_name: Option<String>,
    /// `apps/<pkg>/meta.json`: the full analysis of the app (`AppInfo`).
    pub meta: String,
    /// Relative paths of the APKs; empty when only data was saved.
    pub apks: Vec<String>,
    pub data: Vec<DataArchive>,
    pub permissions: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DataArchive {
    pub part: DataPart,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ItemError {
    pub package: Option<String>,
    /// What failed, e.g. `apk:base.apk`, `data:ce`, `forceStop`.
    pub step: String,
    pub error: Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub format_version: u32,
    pub app_version: String,
    /// RFC 3339, UTC.
    pub created_at: String,
    pub name: String,
    pub source: SourceDevice,
    pub apps: Vec<ManifestApp>,
    /// Accounts to sign in again on the target (point 17); names and types only.
    pub accounts: Vec<Account>,
    /// Apps with Android Keystore keys (point 18).
    pub keystore_packages: Vec<String>,
    /// Resto tab items (messages, contacts, Wi-Fi, files…).
    #[serde(default)]
    pub other: Vec<crate::other::backup::OtherEntry>,
    pub files: Vec<FileEntry>,
    pub errors: Vec<ItemError>,
    /// False when the backup was cancelled or the phone went away.
    pub complete: bool,
}

impl Manifest {
    pub fn read(path: &Path) -> Result<Manifest> {
        let text = std::fs::read_to_string(path)?;
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Manifest> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Version {
            format_version: u32,
        }
        let bad = |e: serde_json::Error| Error::UnexpectedOutput { what: format!("{MANIFEST}: {e}") };
        let v: Version = serde_json::from_str(text).map_err(bad)?;
        if v.format_version > FORMAT_VERSION {
            return Err(Error::UnsupportedFormat { version: v.format_version });
        }
        serde_json::from_str(text).map_err(bad)
    }

    pub fn write(&self, dir: &Path) -> Result<()> {
        write_json(&dir.join(MANIFEST), self)
    }
}

pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(value).map_err(|e| Error::Io { reason: e.to_string() })?;
    std::fs::write(path, text)?;
    Ok(())
}

/// Relative path inside a backup, always with `/`.
pub fn rel(parts: &[&str]) -> String {
    parts.join("/")
}

#[cfg(test)]
mod name_tests {
    use super::*;

    #[test]
    fn validates_backup_names() {
        for ok in ["Pixel_2026-10-14_1400", "Copia 1.5", "v1.2"] {
            assert!(validate_name(ok).is_ok(), "{ok}");
        }
        for bad in ["", ".", "..", "../x", "/etc", "a/b", "a\\b", "C:x", "a*b", "x.", "x ", "con", "NUL.txt", "com1", "a\nb"] {
            assert!(validate_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn suffix_keeps_dots() {
        assert_eq!(packed_path(Path::new("/x/v1.3")), Path::new("/x/v1.3.adbkeep"));
    }
}
