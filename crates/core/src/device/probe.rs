//! Reading what the UI shows about a phone (ui-spec §1 device bar, §2 connection panel).

use std::collections::HashMap;
use std::time::Duration;

use serde::Serialize;

use super::Device;
use crate::analysis::split_sections;
use crate::root::RootMethod;
use crate::{Error, Result};

/// Tools checked in advanced mode (ui-spec §2).
pub const TOOLS: &[&str] = &["tar", "abx2xml", "restorecon", "content", "cmd", "sqlite3"];

/// Magisk asks on the phone and denies by itself after its own timeout (10 s by default).
const SU_TIMEOUT: Duration = Duration::from_secs(40);

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub serial: String,
    pub manufacturer: String,
    pub model: String,
    pub android_release: String,
    pub sdk: u32,
    pub abis: Vec<String>,
    /// `user`, `userdebug` or `eng`.
    pub build_type: String,
    pub build_id: String,
    pub battery: Option<Battery>,
    pub storage: Option<Storage>,
    /// Raw state of the current user from `dumpsys user` (e.g. `RUNNING_UNLOCKED`).
    pub user_state: Option<String>,
    pub users: Vec<UserEntry>,
    pub tools: Vec<Tool>,
    /// adbd already runs as root (`adb root`).
    pub adbd_root: bool,
    /// `adb root` could be enabled: debuggable build or LineageOS-style rooted debugging.
    pub adb_root_possible: bool,
    pub su_path: Option<String>,
}

impl DeviceInfo {
    /// Credential-encrypted (CE) app data is readable only once the user has unlocked since boot.
    pub fn user_unlocked(&self) -> bool {
        self.user_state.as_deref().is_none_or(|s| s == "RUNNING_UNLOCKED")
    }

    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.iter().any(|t| t.name == name && t.path.is_some())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Battery {
    pub level: u32,
    pub plugged: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Storage {
    pub free_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UserEntry {
    pub id: u32,
    pub name: String,
    pub running: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub name: String,
    pub path: Option<String>,
}

/// Result of checking root (ui-spec §2: sin root / root denegado / listo).
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RootStatus {
    /// The method in use; `None` without root.
    pub method: Option<RootMethod>,
    /// Every method that works, for the advanced-mode choice.
    pub available: Vec<RootMethod>,
    /// `su` was found but access was refused (or the prompt timed out).
    pub su_denied: bool,
    /// e.g. "Magisk 30700", "KernelSU".
    pub su_provider: Option<String>,
}

const PROBE_SCRIPT: &str = "echo @@getprop; getprop; \
echo @@battery; dumpsys battery; \
echo @@statfs; stat -f -c '%S %a %b' /data; \
echo @@uid; id -u; \
echo @@tools; for t in tar abx2xml restorecon content cmd sqlite3; do echo \"$t=$(command -v $t)\"; done; \
echo @@su; command -v su; \
echo @@users; pm list users; \
echo @@dumpsysuser; dumpsys user";

/// Everything but root, in one adb call. Safe to repeat (it only reads).
pub async fn probe(device: &Device) -> Result<DeviceInfo> {
    let out = device.shell_timeout(PROBE_SCRIPT, Duration::from_secs(30)).await?;
    parse_probe(device.serial(), &out.stdout_str())
}

/// Checks root. With `su` this may show a prompt on the phone and wait for the user.
pub async fn probe_root(device: &Device, info: &DeviceInfo) -> Result<RootStatus> {
    let mut status = RootStatus { method: None, available: vec![], su_denied: false, su_provider: None };
    if info.adbd_root {
        status.available.push(RootMethod::AdbRoot);
    }
    if info.su_path.is_some() {
        let version = device.shell_timeout("su -v; su -V", Duration::from_secs(10)).await;
        status.su_provider = version.ok().and_then(|o| su_provider(&o.stdout_str()));
        // With adbd as root, `su` is redundant; skip it so no prompt appears.
        if !info.adbd_root {
            match device.shell_timeout("su -c 'id -u'", SU_TIMEOUT).await {
                Ok(out) if out.success() && out.stdout_str().trim() == "0" => status.available.push(RootMethod::Su),
                Ok(_) | Err(Error::Timeout { .. }) => status.su_denied = true,
                Err(e) => return Err(e),
            }
        }
    }
    status.method = status.available.first().copied();
    Ok(status)
}

fn parse_probe(serial: &str, out: &str) -> Result<DeviceInfo> {
    let sections = split_sections(out);
    let section = |name: &str| sections.get(name).map(String::as_str).unwrap_or_default();
    let props = parse_getprop(section("getprop"));
    if props.is_empty() {
        return Err(Error::UnexpectedOutput { what: "getprop".into() });
    }
    let prop = |k: &str| props.get(k).cloned().unwrap_or_default();
    let tools = section("tools")
        .lines()
        .filter_map(|l| l.split_once('='))
        .map(|(name, path)| Tool { name: name.to_string(), path: Some(path.trim()).filter(|p| !p.is_empty()).map(str::to_string) })
        .collect();
    let su_path = section("su").lines().next().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    let debuggable = prop("ro.debuggable") == "1";
    let lineage = props.contains_key("ro.lineage.version");
    let adbd_root = section("uid").trim() == "0";
    Ok(DeviceInfo {
        serial: serial.to_string(),
        manufacturer: prop("ro.product.manufacturer"),
        model: prop("ro.product.model"),
        android_release: prop("ro.build.version.release"),
        sdk: prop("ro.build.version.sdk").parse().unwrap_or(0),
        abis: prop("ro.product.cpu.abilist").split(',').filter(|s| !s.is_empty()).map(str::to_string).collect(),
        build_type: prop("ro.build.type"),
        build_id: prop("ro.build.display.id"),
        battery: parse_battery(section("battery")),
        storage: parse_statfs(section("statfs")),
        user_state: parse_current_user_state(section("dumpsysuser")),
        users: parse_pm_users(section("users")),
        tools,
        adbd_root,
        adb_root_possible: !adbd_root && (debuggable || lineage),
        su_path,
    })
}

/// `[key]: [value]` lines.
pub fn parse_getprop(out: &str) -> HashMap<String, String> {
    out.lines()
        .filter_map(|l| {
            let (k, v) = l.trim().split_once("]: [")?;
            Some((k.strip_prefix('[')?.to_string(), v.strip_suffix(']')?.to_string()))
        })
        .collect()
}

fn parse_battery(out: &str) -> Option<Battery> {
    let mut level = None;
    let mut plugged = false;
    for line in out.lines() {
        let Some((k, v)) = line.trim().split_once(": ") else { continue };
        match k {
            "level" => level = v.trim().parse().ok(),
            "AC powered" | "USB powered" | "Wireless powered" | "Dock powered" => plugged |= v.trim() == "true",
            _ => {}
        }
    }
    Some(Battery { level: level?, plugged })
}

/// `stat -f -c '%S %a %b'`: block size, available blocks, total blocks.
fn parse_statfs(out: &str) -> Option<Storage> {
    let nums: Vec<u64> = out.split_whitespace().filter_map(|n| n.parse().ok()).collect();
    match nums[..] {
        [bsize, avail, total] => Some(Storage { free_bytes: bsize * avail, total_bytes: bsize * total }),
        _ => None,
    }
}

/// `State:` of the current user's `UserInfo{…}` block in `dumpsys user`.
fn parse_current_user_state(out: &str) -> Option<String> {
    let current = out
        .lines()
        .find_map(|l| l.trim().strip_prefix("Current user:"))
        .and_then(|v| v.trim().parse::<u32>().ok())
        .unwrap_or(0);
    let header = format!("UserInfo{{{current}:");
    let mut in_block = false;
    for line in out.lines() {
        let line = line.trim();
        if line.starts_with("UserInfo{") {
            in_block = line.starts_with(&header);
        } else if in_block && let Some(state) = line.strip_prefix("State:") {
            return Some(state.trim().to_string());
        }
    }
    None
}

/// `UserInfo{0:Owner:4c13} running` lines of `pm list users`.
fn parse_pm_users(out: &str) -> Vec<UserEntry> {
    out.lines()
        .filter_map(|l| {
            let l = l.trim();
            let inner = l.strip_prefix("UserInfo{")?;
            let (inner, rest) = inner.split_once('}')?;
            let mut parts = inner.splitn(2, ':');
            let id = parts.next()?.parse().ok()?;
            let name = parts.next()?.rsplit_once(':').map(|(n, _flags)| n).unwrap_or_default().to_string();
            Some(UserEntry { id, name, running: rest.contains("running") })
        })
        .collect()
}

/// `su -v` + `su -V` output → provider name and version.
fn su_provider(out: &str) -> Option<String> {
    let mut lines = out.lines().map(str::trim).filter(|l| !l.is_empty());
    let v = lines.next()?;
    let code = lines.next().filter(|c| c.chars().all(|ch| ch.is_ascii_digit()));
    let name = if v.contains("MAGISK") {
        "Magisk"
    } else if v.contains("KernelSU") {
        "KernelSU"
    } else if v.contains("APatch") {
        "APatch"
    } else {
        return Some(v.to_string());
    };
    Some(match code {
        Some(c) => format!("{name} {c}"),
        None => name.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/pixel10pro-a16");

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!("{FIXTURES}/{name}")).unwrap()
    }

    fn pixel_probe_output() -> String {
        format!(
            "@@getprop\n{}@@battery\n{}@@statfs\n4096 23925656 28645653\n@@uid\n2000\n@@tools\n\
             tar=/system/bin/tar\nabx2xml=/system/bin/abx2xml\nrestorecon=/system/bin/restorecon\n\
             content=/system/bin/content\ncmd=/system/bin/cmd\nsqlite3=\n@@su\n/system_ext/bin/su\n\
             @@users\n{}@@dumpsysuser\n{}",
            fixture("getprop.txt"),
            fixture("dumpsys-battery.txt"),
            fixture("pm-list-users.txt"),
            fixture("dumpsys-user.txt"),
        )
    }

    #[test]
    fn parses_pixel_probe() {
        let info = parse_probe("59021FDCH001KW", &pixel_probe_output()).unwrap();
        assert_eq!(info.manufacturer, "Google");
        assert_eq!(info.model, "Pixel 10 Pro");
        assert_eq!(info.android_release, "16");
        assert_eq!(info.sdk, 36);
        assert_eq!(info.abis, ["arm64-v8a"]);
        assert_eq!(info.build_type, "user");
        assert_eq!(info.battery, Some(Battery { level: 73, plugged: true }));
        assert_eq!(info.storage.as_ref().unwrap().free_bytes, 4096 * 23925656);
        assert_eq!(info.user_state.as_deref(), Some("RUNNING_UNLOCKED"));
        assert!(info.user_unlocked());
        assert_eq!(info.users, [UserEntry { id: 0, name: "Propietario".into(), running: true }]);
        assert!(info.has_tool("tar"));
        assert!(!info.has_tool("sqlite3"));
        assert!(!info.adbd_root);
        assert!(!info.adb_root_possible);
        assert_eq!(info.su_path.as_deref(), Some("/system_ext/bin/su"));
    }

    #[test]
    fn locked_user_and_lineage_adb_root() {
        let out = "@@getprop\n[ro.lineage.version]: [23.2]\n[ro.build.version.sdk]: [36]\n\
                   @@uid\n2000\n@@dumpsysuser\nCurrent user: 0\nUsers:\n  UserInfo{0:null:c13} serialNo=0\n    State: RUNNING_LOCKED\n";
        let info = parse_probe("x", out).unwrap();
        assert!(!info.user_unlocked());
        assert!(info.adb_root_possible);
        assert_eq!(info.battery, None);
        assert_eq!(info.su_path, None);
    }

    #[test]
    fn state_of_secondary_current_user() {
        let out = "Current user: 10\nUsers:\n  UserInfo{0:null:c13}\n    State: RUNNING_UNLOCKED\n  UserInfo{10:Work:30}\n    State: RUNNING_LOCKED\n";
        assert_eq!(parse_current_user_state(out).as_deref(), Some("RUNNING_LOCKED"));
    }

    #[test]
    fn su_providers() {
        assert_eq!(su_provider("14ea5cfb:MAGISKSU\n30700\n").as_deref(), Some("Magisk 30700"));
        assert_eq!(su_provider("v1.0.3:KernelSU\n").as_deref(), Some("KernelSU"));
        assert_eq!(su_provider(""), None);
    }
}
