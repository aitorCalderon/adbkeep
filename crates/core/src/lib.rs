//! Platform-independent core of adbkeep: talking to the phone over adb, analysis,
//! backup/restore runners and the backup format. The Tauri app is a thin layer on top.

pub mod abx;
pub mod adb;
pub mod analysis;
pub mod archive;
pub mod backup;
pub mod compat;
pub mod device;
pub mod error;
pub mod format;
pub mod helper;
pub mod library;
pub mod log;
pub mod other;
pub mod pause;
pub mod restore;
pub mod root;
pub mod stream;
pub mod viewer;

pub use error::{Error, Result};
