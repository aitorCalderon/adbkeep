//! Keeps track of connected phones and of the selected one's state, notifying every change.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tokio::task::JoinHandle;

use super::probe::{self, DeviceInfo, RootStatus};
use super::state::{ConnectionState, ProbeState, connection_state};
use super::{Device, track};
use crate::adb::{Adb, DeviceEntry, DeviceState};
use crate::root::RootMethod;
use crate::{Error, Result};

const RETRACK_DELAY: Duration = Duration::from_secs(2);
/// Battery, free space and lock state are refreshed this often while connected…
const REFRESH_EVERY: Duration = Duration::from_secs(20);
/// …and faster while waiting for the user to unlock.
const REFRESH_LOCKED_EVERY: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSnapshot {
    pub devices: Vec<DeviceEntry>,
    pub selected: Option<String>,
    pub state: ConnectionState,
    pub info: Option<DeviceInfo>,
    pub root: Option<RootStatus>,
}

type Notify = Arc<dyn Fn(DeviceSnapshot) + Send + Sync>;

#[derive(Default)]
struct Inner {
    adb: Option<Adb>,
    devices: Vec<DeviceEntry>,
    selected: Option<String>,
    probe: ProbeState,
    /// Root method chosen in advanced mode, if different from the default.
    preferred_root: Option<RootMethod>,
    adb_root_requested: HashSet<String>,
    /// Bumped whenever the selection or adb changes, so stale probe results are dropped.
    generation: u64,
    tasks: Vec<JoinHandle<()>>,
}

impl Inner {
    fn selected_entry(&self) -> Option<&DeviceEntry> {
        let serial = self.selected.as_ref()?;
        self.devices.iter().find(|d| &d.serial == serial)
    }

    fn snapshot(&self) -> DeviceSnapshot {
        let state = if self.adb.is_none() {
            ConnectionState::NoAdb
        } else {
            let requested = self.selected.as_ref().is_some_and(|s| self.adb_root_requested.contains(s));
            connection_state(self.selected_entry(), requested, &self.probe)
        };
        DeviceSnapshot {
            devices: self.devices.clone(),
            selected: self.selected.clone(),
            state,
            info: self.probe.info().cloned(),
            root: self.probe.root().cloned(),
        }
    }
}

#[derive(Clone)]
pub struct DeviceManager {
    inner: Arc<Mutex<Inner>>,
    notify: Notify,
}

impl DeviceManager {
    pub fn new(notify: impl Fn(DeviceSnapshot) + Send + Sync + 'static) -> Self {
        Self { inner: Arc::default(), notify: Arc::new(notify) }
    }

    pub fn snapshot(&self) -> DeviceSnapshot {
        self.inner.lock().unwrap().snapshot()
    }

    /// The selected phone once it is ready, with its root method.
    pub fn ready_device(&self) -> Result<Device> {
        let inner = self.inner.lock().unwrap();
        let snap = inner.snapshot();
        match (snap.state, inner.adb.clone(), snap.selected, snap.root) {
            (ConnectionState::Ready, Some(adb), Some(serial), Some(root)) => {
                Ok(Device::new(adb, serial).with_root(root.method))
            }
            (_, _, serial, _) => Err(Error::DeviceUnavailable { serial: serial.unwrap_or_default() }),
        }
    }

    /// Starts (or restarts) tracking with a new adb; `None` when adb is not usable.
    pub fn set_adb(&self, adb: Option<Adb>) {
        {
            let mut inner = self.inner.lock().unwrap();
            for t in inner.tasks.drain(..) {
                t.abort();
            }
            inner.generation += 1;
            inner.devices.clear();
            inner.probe = ProbeState::NotStarted;
            inner.adb = adb.clone();
            if let Some(adb) = adb {
                let tracker = tokio::spawn(self.clone().track_loop(adb));
                let refresher = tokio::spawn(self.clone().refresh_loop());
                inner.tasks.extend([tracker, refresher]);
            }
        }
        self.emit();
    }

    pub fn select(&self, serial: String) {
        {
            let mut inner = self.inner.lock().unwrap();
            if inner.selected.as_ref() == Some(&serial) {
                return;
            }
            inner.selected = Some(serial);
            inner.generation += 1;
            inner.probe = ProbeState::NotStarted;
        }
        self.maybe_probe();
        self.emit();
    }

    /// Probes the selected phone again (Retry button).
    pub fn reprobe(&self) {
        {
            let mut inner = self.inner.lock().unwrap();
            inner.generation += 1;
            inner.probe = ProbeState::NotStarted;
        }
        self.maybe_probe();
        self.emit();
    }

    /// Advanced mode: use another available root method.
    pub fn set_root_method(&self, method: RootMethod) {
        {
            let mut inner = self.inner.lock().unwrap();
            inner.preferred_root = Some(method);
            if let ProbeState::Done(_, root) = &mut inner.probe
                && root.available.contains(&method)
            {
                root.method = Some(method);
            }
        }
        self.emit();
    }

    /// Runs `adb root`. adbd restarts and the phone asks to authorize the PC again (ui-spec §2).
    pub async fn enable_adb_root(&self) -> Result<()> {
        let (adb, serial) = {
            let mut inner = self.inner.lock().unwrap();
            let serial = inner.selected.clone().ok_or(Error::DeviceUnavailable { serial: String::new() })?;
            inner.adb_root_requested.insert(serial.clone());
            (inner.adb.clone().ok_or(Error::AdbNotFound)?, serial)
        };
        let out = Device::new(adb, serial).adb_run(&["root"], Some(Duration::from_secs(30))).await?;
        out.ok()?;
        self.reprobe();
        Ok(())
    }

    fn emit(&self) {
        let snap = self.snapshot();
        (self.notify)(snap);
    }

    async fn track_loop(self, adb: Adb) {
        loop {
            if let Ok((mut child, mut stdout)) = track::spawn(&adb) {
                while let Ok(Some(devices)) = track::next_update(&mut stdout).await {
                    self.on_devices(devices);
                }
                let _ = child.kill().await;
            }
            self.on_devices(vec![]);
            tokio::time::sleep(RETRACK_DELAY).await;
        }
    }

    fn on_devices(&self, devices: Vec<DeviceEntry>) {
        {
            let mut inner = self.inner.lock().unwrap();
            let still_there = inner.selected.as_ref().is_some_and(|s| devices.iter().any(|d| &d.serial == s));
            if !still_there {
                let first = devices.iter().find(|d| d.state == DeviceState::Device).or(devices.first());
                let new = first.map(|d| d.serial.clone());
                if new != inner.selected {
                    inner.selected = new;
                    inner.generation += 1;
                    inner.probe = ProbeState::NotStarted;
                }
            }
            inner.devices = devices;
            // Whenever the phone is not usable, probe again from scratch once it is.
            let selected_ready = inner.selected_entry().is_some_and(|d| d.state == DeviceState::Device);
            if !selected_ready && !matches!(inner.probe, ProbeState::NotStarted) {
                inner.generation += 1;
                inner.probe = ProbeState::NotStarted;
            }
            if selected_ready && let Some(s) = inner.selected.clone() {
                inner.adb_root_requested.remove(&s);
            }
        }
        self.maybe_probe();
        self.emit();
    }

    /// Starts probing the selected phone if it is connected and not probed yet.
    fn maybe_probe(&self) {
        let (device, generation) = {
            let mut inner = self.inner.lock().unwrap();
            let ready = inner.selected_entry().is_some_and(|d| d.state == DeviceState::Device);
            if !ready || !matches!(inner.probe, ProbeState::NotStarted) {
                return;
            }
            let (Some(adb), Some(serial)) = (inner.adb.clone(), inner.selected.clone()) else { return };
            inner.probe = ProbeState::Probing;
            (Device::new(adb, serial), inner.generation)
        };
        let this = self.clone();
        tokio::spawn(async move { this.probe_task(device, generation).await });
    }

    async fn probe_task(&self, device: Device, generation: u64) {
        let info = match probe::probe(&device).await {
            Ok(info) => info,
            Err(e) => return self.set_probe(generation, ProbeState::Failed(e)),
        };
        self.set_probe(generation, ProbeState::RootPending(info.clone()));
        let state = match probe::probe_root(&device, &info).await {
            Ok(mut root) => {
                let preferred = self.inner.lock().unwrap().preferred_root;
                if let Some(p) = preferred.filter(|p| root.available.contains(p)) {
                    root.method = Some(p);
                }
                ProbeState::Done(info, root)
            }
            Err(e) => ProbeState::Failed(e),
        };
        self.set_probe(generation, state);
    }

    fn set_probe(&self, generation: u64, probe: ProbeState) {
        {
            let mut inner = self.inner.lock().unwrap();
            if inner.generation != generation {
                return;
            }
            inner.probe = probe;
        }
        self.emit();
    }

    /// Refreshes battery, free space and lock state of a probed phone; root is not asked again.
    async fn refresh_loop(self) {
        loop {
            let locked = matches!(self.snapshot().state, ConnectionState::Locked);
            tokio::time::sleep(if locked { REFRESH_LOCKED_EVERY } else { REFRESH_EVERY }).await;
            let (device, generation) = {
                let inner = self.inner.lock().unwrap();
                let (ProbeState::Done(..), Some(adb), Some(serial)) = (&inner.probe, inner.adb.clone(), inner.selected.clone())
                else {
                    continue;
                };
                (Device::new(adb, serial), inner.generation)
            };
            if let Ok(info) = probe::probe(&device).await {
                let changed = {
                    let mut inner = self.inner.lock().unwrap();
                    let current = inner.generation == generation;
                    match &mut inner.probe {
                        ProbeState::Done(old, _) if current && *old != info => {
                            *old = info;
                            true
                        }
                        _ => false,
                    }
                };
                if changed {
                    self.emit();
                }
            }
        }
    }
}
