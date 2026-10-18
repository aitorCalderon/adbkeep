use serde::{Deserialize, Serialize};

/// Errors never carry UI text: the frontend translates `code` and its parameters.
#[derive(Debug, Clone, PartialEq, thiserror::Error, Serialize, Deserialize, specta::Type)]
#[serde(tag = "code", rename_all = "camelCase", rename = "CoreError")]
pub enum Error {
    #[error("adb not found")]
    AdbNotFound,
    #[error("adb not usable at {path}: {reason}")]
    AdbNotUsable { path: String, reason: String },
    #[error("failed to run {program}: {reason}")]
    Spawn { program: String, reason: String },
    #[error("command failed with exit code {exit_code:?}: {stderr}")]
    CommandFailed { exit_code: Option<i32>, stderr: String },
    #[error("timed out after {seconds} s")]
    Timeout { seconds: u32 },
    /// The phone went away or is not in the `device` state.
    #[error("device {serial} not available")]
    DeviceUnavailable { serial: String },
    /// A root command was attempted without root access on the phone.
    #[error("root not available")]
    RootUnavailable,
    #[error("unexpected output from {what}")]
    UnexpectedOutput { what: String },
    #[error("backup format version {version} is newer than this app supports")]
    UnsupportedFormat { version: u32 },
    #[error("not enough space: {needed} bytes needed, {available} available")]
    NotEnoughSpace { needed: u64, available: u64 },
    /// Another backup or restore is running.
    #[error("another job is running")]
    Busy,
    /// Messages, contacts and the like need the on-device helper, which is off or unavailable.
    #[error("the on-device helper is needed")]
    HelperRequired,
    /// Not possible (yet) for this item or Android version.
    #[error("not supported")]
    NotSupported,
    /// The pack is encrypted and its password has not been given this session.
    #[error("password required")]
    PasswordRequired,
    #[error("wrong password")]
    WrongPassword,
    /// The disk on the PC filled up while writing.
    #[error("disk full")]
    DiskFull,
    /// Resuming a backup on a phone other than the one it was made from.
    #[error("the backup was made from {expected}")]
    WrongDevice { expected: String },
    #[error("cancelled")]
    Cancelled,
    #[error("database error: {reason}")]
    Database { reason: String },
    #[error("I/O error: {reason}")]
    Io { reason: String },
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::StorageFull {
            return Error::DiskFull;
        }
        Error::Io { reason: e.to_string() }
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
