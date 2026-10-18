//! Parsing `adb devices -l`.

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum DeviceState {
    Device,
    Unauthorized,
    Offline,
    /// The PC lacks permission to open the USB device (udev rules on Linux).
    NoPermissions,
    Recovery,
    Sideload,
    Bootloader,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DeviceEntry {
    pub serial: String,
    pub state: DeviceState,
    /// `model:` as reported by adb, with `_` instead of spaces (e.g. `SM_S911B`).
    pub model: Option<String>,
    pub product: Option<String>,
    pub device: Option<String>,
    pub transport_id: Option<String>,
}

pub fn parse_devices(output: &str) -> Vec<DeviceEntry> {
    output
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("List of devices") && !l.starts_with('*'))
        .filter_map(parse_line)
        .collect()
}

fn parse_line(line: &str) -> Option<DeviceEntry> {
    let mut words = line.split_whitespace();
    let serial = words.next()?.to_string();
    let rest: Vec<&str> = words.collect();
    let state = match rest.first().copied()? {
        "device" => DeviceState::Device,
        "unauthorized" => DeviceState::Unauthorized,
        "offline" => DeviceState::Offline,
        "no" if rest.get(1) == Some(&"permissions") => DeviceState::NoPermissions,
        "recovery" => DeviceState::Recovery,
        "sideload" => DeviceState::Sideload,
        "bootloader" => DeviceState::Bootloader,
        other => DeviceState::Other(other.to_string()),
    };
    let field = |key: &str| {
        rest.iter()
            .find_map(|w| w.strip_prefix(key).and_then(|v| v.strip_prefix(':')))
            .map(str::to_string)
    };
    Some(DeviceEntry {
        model: field("model"),
        product: field("product"),
        device: field("device"),
        transport_id: field("transport_id"),
        serial,
        state,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_states_and_fields() {
        let out = "* daemon not running; starting now at tcp:5037\n\
                   * daemon started successfully\n\
                   List of devices attached\n\
                   R5CW1234567    device usb:1-1 product:dm1qxeea model:SM_S911B device:dm1q transport_id:3\n\
                   emulator-5554  unauthorized transport_id:1\n\
                   0123ABCD       no permissions (missing udev rules? user is in the plugdev group); see [http://developer.android.com/tools/device.html] usb:1-2 transport_id:4\n\
                   \n";
        let devs = parse_devices(out);
        assert_eq!(devs.len(), 3);
        assert_eq!(devs[0].serial, "R5CW1234567");
        assert_eq!(devs[0].state, DeviceState::Device);
        assert_eq!(devs[0].model.as_deref(), Some("SM_S911B"));
        assert_eq!(devs[0].transport_id.as_deref(), Some("3"));
        assert_eq!(devs[1].state, DeviceState::Unauthorized);
        assert_eq!(devs[1].model, None);
        assert_eq!(devs[2].state, DeviceState::NoPermissions);
        assert_eq!(devs[2].transport_id.as_deref(), Some("4"));
    }
}
