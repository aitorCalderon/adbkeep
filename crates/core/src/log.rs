//! Log of every adb/shell command run, shown in the advanced-mode log panel (ui-spec §1).

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

/// Captured output beyond this many bytes is cut, keeping the log light.
const MAX_CAPTURE: usize = 16 * 1024;
const MAX_ENTRIES: usize = 2000;

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub id: u32,
    /// Unix time in milliseconds.
    pub started_at: u64,
    pub duration_ms: u32,
    pub program: String,
    pub args: Vec<String>,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    /// Bytes of stdout when it was binary and streamed elsewhere instead of captured.
    pub streamed_bytes: Option<u64>,
    /// Which job ran it (e.g. a backup), so its own session log leaves out other commands.
    pub tag: Option<String>,
}

type Listener = Box<dyn Fn(&LogEntry) + Send + Sync>;

#[derive(Default)]
pub struct CommandLog {
    next_id: AtomicU64,
    entries: Mutex<VecDeque<LogEntry>>,
    listener: Mutex<Option<Listener>>,
}

impl CommandLog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Called for every new entry (the app forwards them to the UI).
    pub fn set_listener(&self, f: impl Fn(&LogEntry) + Send + Sync + 'static) {
        *self.listener.lock().unwrap() = Some(Box::new(f));
    }

    pub fn next_id(&self) -> u32 {
        self.next_id.fetch_add(1, Ordering::Relaxed) as u32
    }

    pub fn record(&self, entry: LogEntry) {
        if let Some(f) = self.listener.lock().unwrap().as_ref() {
            f(&entry);
        }
        let mut entries = self.entries.lock().unwrap();
        if entries.len() == MAX_ENTRIES {
            entries.pop_front();
        }
        entries.push_back(entry);
    }

    pub fn entries(&self) -> Vec<LogEntry> {
        self.entries.lock().unwrap().iter().cloned().collect()
    }

    pub fn clear(&self) {
        self.entries.lock().unwrap().clear();
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

/// Lossy UTF-8 of `bytes`, cut to [`MAX_CAPTURE`] bytes.
pub fn capture(bytes: &[u8]) -> String {
    if bytes.len() <= MAX_CAPTURE {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let mut s = String::from_utf8_lossy(&bytes[..MAX_CAPTURE]).into_owned();
    s.push_str(&format!("\n… ({} bytes more)", bytes.len() - MAX_CAPTURE));
    s
}
