//! Connection states of ui-spec §2, derived from adb's view of the phone and the probe results.

use serde::Serialize;

use super::probe::{DeviceInfo, RootStatus};
use crate::Error;
use crate::adb::{DeviceEntry, DeviceState};

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ConnectionState {
    /// adb itself is missing or broken (Preferences).
    NoAdb,
    NoDevice,
    /// Linux without udev rules: the PC may not open the USB device.
    NoPermissions,
    Unauthorized,
    /// `adb root` restarted adbd and the phone asks to authorize the PC again.
    ReauthorizeAfterAdbRoot,
    /// adb sees the phone but it is offline, in recovery, etc.
    Connecting,
    Probing,
    /// Waiting for the user to answer the root prompt on the phone.
    RootPending,
    NoRoot,
    RootDenied,
    /// The user has not unlocked since boot: CE data is not readable.
    Locked,
    Ready,
    Failed { error: Error },
}

/// Progress of probing the selected phone.
#[derive(Debug, Clone, Default)]
pub enum ProbeState {
    #[default]
    NotStarted,
    Probing,
    RootPending(DeviceInfo),
    Done(DeviceInfo, RootStatus),
    Failed(Error),
}

impl ProbeState {
    pub fn info(&self) -> Option<&DeviceInfo> {
        match self {
            ProbeState::RootPending(info) | ProbeState::Done(info, _) => Some(info),
            _ => None,
        }
    }

    pub fn root(&self) -> Option<&RootStatus> {
        match self {
            ProbeState::Done(_, root) => Some(root),
            _ => None,
        }
    }
}

pub fn connection_state(entry: Option<&DeviceEntry>, adb_root_requested: bool, probe: &ProbeState) -> ConnectionState {
    let Some(entry) = entry else { return ConnectionState::NoDevice };
    match entry.state {
        DeviceState::Device => {}
        DeviceState::Unauthorized if adb_root_requested => return ConnectionState::ReauthorizeAfterAdbRoot,
        DeviceState::Unauthorized => return ConnectionState::Unauthorized,
        DeviceState::NoPermissions => return ConnectionState::NoPermissions,
        _ => return ConnectionState::Connecting,
    }
    match probe {
        ProbeState::NotStarted | ProbeState::Probing => ConnectionState::Probing,
        ProbeState::RootPending(_) => ConnectionState::RootPending,
        ProbeState::Failed(error) => ConnectionState::Failed { error: error.clone() },
        ProbeState::Done(info, root) => match root.method {
            None if root.su_denied => ConnectionState::RootDenied,
            None => ConnectionState::NoRoot,
            Some(_) if !info.user_unlocked() => ConnectionState::Locked,
            Some(_) => ConnectionState::Ready,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::root::RootMethod;

    fn entry(state: DeviceState) -> DeviceEntry {
        DeviceEntry { serial: "s".into(), state, model: None, product: None, device: None, transport_id: None }
    }

    fn info(user_state: &str) -> DeviceInfo {
        DeviceInfo {
            serial: "s".into(),
            manufacturer: String::new(),
            model: String::new(),
            android_release: "16".into(),
            sdk: 36,
            abis: vec![],
            build_type: "user".into(),
            build_id: String::new(),
            battery: None,
            storage: None,
            user_state: Some(user_state.into()),
            users: vec![],
            tools: vec![],
            adbd_root: false,
            adb_root_possible: false,
            su_path: None,
        }
    }

    fn root(method: Option<RootMethod>, denied: bool) -> RootStatus {
        RootStatus { method, available: method.into_iter().collect(), su_denied: denied, su_provider: None }
    }

    #[test]
    fn adb_states() {
        let p = ProbeState::NotStarted;
        assert_eq!(connection_state(None, false, &p), ConnectionState::NoDevice);
        assert_eq!(connection_state(Some(&entry(DeviceState::Unauthorized)), false, &p), ConnectionState::Unauthorized);
        assert_eq!(
            connection_state(Some(&entry(DeviceState::Unauthorized)), true, &p),
            ConnectionState::ReauthorizeAfterAdbRoot
        );
        assert_eq!(connection_state(Some(&entry(DeviceState::Offline)), false, &p), ConnectionState::Connecting);
        assert_eq!(connection_state(Some(&entry(DeviceState::Device)), false, &p), ConnectionState::Probing);
    }

    #[test]
    fn root_and_lock_states() {
        let dev = entry(DeviceState::Device);
        let s = |p: ProbeState| connection_state(Some(&dev), false, &p);
        let unlocked = info("RUNNING_UNLOCKED");
        assert_eq!(s(ProbeState::RootPending(unlocked.clone())), ConnectionState::RootPending);
        assert_eq!(s(ProbeState::Done(unlocked.clone(), root(None, false))), ConnectionState::NoRoot);
        assert_eq!(s(ProbeState::Done(unlocked.clone(), root(None, true))), ConnectionState::RootDenied);
        assert_eq!(s(ProbeState::Done(unlocked, root(Some(RootMethod::Su), false))), ConnectionState::Ready);
        assert_eq!(
            s(ProbeState::Done(info("RUNNING_LOCKED"), root(Some(RootMethod::Su), false))),
            ConnectionState::Locked
        );
    }
}
