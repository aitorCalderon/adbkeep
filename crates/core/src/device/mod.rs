//! A connected phone: running shell commands on it, probing it and tracking connections.

pub mod manager;
pub mod probe;
pub mod state;
pub mod track;

use std::time::Duration;

use crate::adb::{Adb, Output};
use crate::root::{RootMethod, as_root};
use crate::{Error, Result};

/// One phone, addressed by its adb serial.
#[derive(Clone)]
pub struct Device {
    adb: Adb,
    serial: String,
    root: Option<RootMethod>,
}

impl Device {
    pub fn new(adb: Adb, serial: impl Into<String>) -> Self {
        Self { adb, serial: serial.into(), root: None }
    }

    pub fn with_root(mut self, root: Option<RootMethod>) -> Self {
        self.root = root;
        self
    }

    /// The same phone, with the log entries of its commands tagged.
    pub fn tagged(&self, tag: &str) -> Self {
        Self { adb: self.adb.tagged(tag), ..self.clone() }
    }

    pub fn serial(&self) -> &str {
        &self.serial
    }

    pub fn adb(&self) -> &Adb {
        &self.adb
    }

    pub fn root(&self) -> Option<RootMethod> {
        self.root
    }

    /// `adb -s <serial> <args>`.
    pub async fn adb_run(&self, args: &[&str], timeout: Option<Duration>) -> Result<Output> {
        let mut full = vec!["-s", self.serial.as_str()];
        full.extend_from_slice(args);
        let out = self.adb.run_timeout(&full, timeout).await?;
        if !out.success() && is_device_gone(&out.stderr) {
            return Err(Error::DeviceUnavailable { serial: self.serial.clone() });
        }
        Ok(out)
    }

    /// Runs `cmd` with the device shell (`/system/bin/sh -c`), as the adb shell user.
    pub async fn shell(&self, cmd: &str) -> Result<Output> {
        self.adb_run(&["shell", cmd], None).await
    }

    pub async fn shell_timeout(&self, cmd: &str, timeout: Duration) -> Result<Output> {
        self.adb_run(&["shell", cmd], Some(timeout)).await
    }

    /// Runs `cmd` as root with the detected root method.
    pub async fn root_shell(&self, cmd: &str) -> Result<Output> {
        let method = self.root.ok_or(Error::RootUnavailable)?;
        self.shell(&as_root(method, cmd)).await
    }
}

/// adb's own errors when the phone is gone or not usable (as opposed to the remote command failing).
fn is_device_gone(stderr: &str) -> bool {
    let s = stderr.trim_start();
    s.starts_with("adb: device") || s.starts_with("error: device") || s.starts_with("error: no devices")
        || s.contains("device offline") || s.contains("device unauthorized")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_adb_device_errors() {
        assert!(is_device_gone("error: device '59021FDCH001KW' not found\n"));
        assert!(is_device_gone("error: device offline"));
        assert!(is_device_gone("adb: device unauthorized.\nThis adb server's $ADB_VENDOR_KEYS is not set"));
        assert!(!is_device_gone("tar: no such file"));
    }
}
