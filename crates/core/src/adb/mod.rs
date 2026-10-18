//! Running the adb binary.

mod devices;
mod locate;

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::process::Command;

pub use devices::{DeviceEntry, DeviceState, parse_devices};
pub use locate::{AdbLocation, AdbSource, adb_file_name, locate};

use crate::log::{CommandLog, LogEntry, capture, now_ms};
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct Output {
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

impl Output {
    pub fn success(&self) -> bool {
        self.exit_code == Some(0)
    }

    pub fn stdout_str(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    /// The output, or [`Error::CommandFailed`] when the exit code is not 0.
    pub fn ok(self) -> Result<Self> {
        if self.success() {
            Ok(self)
        } else {
            Err(Error::CommandFailed { exit_code: self.exit_code, stderr: self.stderr })
        }
    }
}

/// The adb executable, with every invocation recorded in the command log.
#[derive(Clone)]
pub struct Adb {
    path: PathBuf,
    log: Arc<CommandLog>,
    tag: Option<Arc<str>>,
}

impl Adb {
    pub fn new(path: impl Into<PathBuf>, log: Arc<CommandLog>) -> Self {
        Self { path: path.into(), log, tag: None }
    }

    /// The same adb, with its log entries tagged (see [`LogEntry::tag`]).
    pub fn tagged(&self, tag: &str) -> Self {
        Self { tag: Some(tag.into()), ..self.clone() }
    }

    pub fn tag(&self) -> Option<String> {
        self.tag.as_deref().map(str::to_string)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn log(&self) -> &Arc<CommandLog> {
        &self.log
    }

    pub(crate) fn command(&self) -> Command {
        let mut cmd = Command::new(&self.path);
        cmd.stdin(Stdio::null()).kill_on_drop(true);
        #[cfg(windows)]
        {
            // CREATE_NO_WINDOW: no console window flashing for every command.
            cmd.creation_flags(0x0800_0000);
        }
        #[cfg(target_os = "linux")]
        // SAFETY: prctl is async-signal-safe and only affects the child being spawned.
        unsafe {
            // Die with the app even when it is killed (otherwise `adb track-devices` lingers).
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
        cmd
    }

    /// Runs `adb <args>` capturing its output; a non-zero exit code is not an error here.
    pub async fn run(&self, args: &[&str]) -> Result<Output> {
        self.run_timeout(args, None).await
    }

    /// Like [`Self::run`], killing adb and failing with [`Error::Timeout`] after `timeout`.
    pub async fn run_timeout(&self, args: &[&str], timeout: Option<Duration>) -> Result<Output> {
        let id = self.log.next_id();
        let started_at = now_ms();
        let start = Instant::now();
        let mut cmd = self.command();
        cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
        let result = match timeout {
            None => cmd.output().await,
            Some(t) => match tokio::time::timeout(t, cmd.output()).await {
                Ok(r) => r,
                Err(_) => {
                    let secs = t.as_secs() as u32;
                    self.log.record(LogEntry {
                        id,
                        started_at,
                        duration_ms: start.elapsed().as_millis() as u32,
                        program: "adb".into(),
                        args: args.iter().map(|s| s.to_string()).collect(),
                        exit_code: None,
                        stdout: String::new(),
                        stderr: format!("timed out after {secs} s"),
                        streamed_bytes: None,
                        tag: self.tag(),
                    });
                    return Err(Error::Timeout { seconds: secs });
                }
            },
        };
        let entry = |exit_code, stdout: &[u8], stderr: &[u8]| LogEntry {
            id,
            started_at,
            duration_ms: start.elapsed().as_millis() as u32,
            program: "adb".into(),
            args: args.iter().map(|s| s.to_string()).collect(),
            exit_code,
            stdout: capture(stdout),
            stderr: capture(stderr),
            streamed_bytes: None,
            tag: self.tag(),
        };
        match result {
            Ok(out) => {
                self.log.record(entry(out.status.code(), &out.stdout, &out.stderr));
                Ok(Output {
                    exit_code: out.status.code(),
                    stdout: out.stdout,
                    stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
                })
            }
            Err(e) => {
                self.log.record(entry(None, b"", e.to_string().as_bytes()));
                Err(Error::Spawn { program: self.path.display().to_string(), reason: e.to_string() })
            }
        }
    }

    /// First line of `adb version`, e.g. "Android Debug Bridge version 1.0.41".
    pub async fn version(&self) -> Result<String> {
        let out = self.run(&["version"]).await?.ok()?;
        Ok(out.stdout_str().lines().next().unwrap_or_default().trim().to_string())
    }

    pub async fn devices(&self) -> Result<Vec<DeviceEntry>> {
        let out = self.run(&["devices", "-l"]).await?.ok()?;
        Ok(parse_devices(&out.stdout_str()))
    }
}
