//! Pausing a running job while the phone is away or locked, and going on once it is back
//! (ui-spec §6 "Móvil desconectado a mitad" and "Móvil bloqueado durante el proceso").

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Serialize;

use crate::backup::JobEvent;
use crate::device::Device;
use crate::device::manager::DeviceManager;
use crate::device::state::ConnectionState;
use crate::{Error, Result};

const POLL: Duration = Duration::from_millis(500);
/// A step that failed because the phone went away is tried again this many times.
pub const MAX_RETRIES: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum PauseReason {
    /// Unplugged, unauthorized or still reconnecting (probing, root prompt).
    Disconnected,
    /// Restarted and not unlocked since: app data (CE) cannot be read or written.
    Locked,
}

/// What the job can do with the phone right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    Ready,
    Paused(PauseReason),
}

/// Tells a running job whether its phone is usable.
pub trait Gate: Send + Sync {
    fn availability(&self, serial: &str) -> Availability;
}

impl Gate for DeviceManager {
    fn availability(&self, serial: &str) -> Availability {
        let snap = self.snapshot();
        if snap.selected.as_deref() != Some(serial) {
            return Availability::Paused(PauseReason::Disconnected);
        }
        match snap.state {
            ConnectionState::Ready => Availability::Ready,
            ConnectionState::Locked => Availability::Paused(PauseReason::Locked),
            _ => Availability::Paused(PauseReason::Disconnected),
        }
    }
}

/// Returns once the phone is usable, emitting `Paused`/`Resumed` around any wait. Without a
/// gate it returns at once. Cancelling while paused gives [`Error::Cancelled`].
pub async fn wait_ready(
    gate: Option<&dyn Gate>,
    serial: &str,
    cancel: &AtomicBool,
    on_event: &mut impl FnMut(JobEvent),
) -> Result<()> {
    let Some(gate) = gate else { return Ok(()) };
    let mut paused: Option<PauseReason> = None;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        match gate.availability(serial) {
            Availability::Ready => {
                if paused.is_some() {
                    on_event(JobEvent::Resumed);
                }
                return Ok(());
            }
            Availability::Paused(reason) => {
                if paused != Some(reason) {
                    on_event(JobEvent::Paused { reason });
                    paused = Some(reason);
                }
            }
        }
        tokio::time::sleep(POLL).await;
    }
}

/// After a step failed: whether the phone is gone (so the step should wait and be retried)
/// rather than the step itself having failed. adb may report the loss as a plain failure of
/// the command, so ask adb directly.
pub async fn device_lost(device: &Device, error: &Error) -> bool {
    match error {
        Error::Cancelled => false,
        Error::DeviceUnavailable { .. } => true,
        _ => match device.adb_run(&["get-state"], Some(Duration::from_secs(5))).await {
            Ok(out) => !(out.success() && out.stdout_str().trim() == "device"),
            Err(_) => true,
        },
    }
}

/// Whether a failed step should be retried: there is a gate to wait on, retries are left and
/// the phone went away.
pub async fn should_retry(gate: Option<&dyn Gate>, device: &Device, error: &Error, attempt: u32) -> bool {
    gate.is_some() && attempt < MAX_RETRIES && device_lost(device, error).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct Scripted(Mutex<Vec<Availability>>);

    impl Gate for Scripted {
        fn availability(&self, _: &str) -> Availability {
            let mut v = self.0.lock().unwrap();
            if v.len() > 1 { v.remove(0) } else { v[0] }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn pauses_until_ready_and_reports_each_reason_once() {
        use Availability::*;
        let gate = Scripted(Mutex::new(vec![
            Paused(PauseReason::Disconnected),
            Paused(PauseReason::Disconnected),
            Paused(PauseReason::Locked),
            Ready,
        ]));
        let mut events = vec![];
        wait_ready(Some(&gate), "s", &AtomicBool::new(false), &mut |e| events.push(e)).await.unwrap();
        assert_eq!(
            events,
            [
                JobEvent::Paused { reason: PauseReason::Disconnected },
                JobEvent::Paused { reason: PauseReason::Locked },
                JobEvent::Resumed
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn cancelling_while_paused() {
        let gate = Scripted(Mutex::new(vec![Availability::Paused(PauseReason::Disconnected)]));
        let r = wait_ready(Some(&gate), "s", &AtomicBool::new(true), &mut |_| {}).await;
        assert_eq!(r, Err(Error::Cancelled));
    }
}
