//! AppOps, special access, default-app roles (point 3) and accounts (point 17).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// AppOps that Settings shows as special app access; the others follow runtime permissions
/// and are restored by granting those.
pub const SPECIAL_OPS: &[&str] = &[
    "MANAGE_EXTERNAL_STORAGE",
    "SYSTEM_ALERT_WINDOW",
    "WRITE_SETTINGS",
    "REQUEST_INSTALL_PACKAGES",
    "GET_USAGE_STATS",
    "PICTURE_IN_PICTURE",
    "SCHEDULE_EXACT_ALARM",
    "MANAGE_MEDIA",
    "RUN_ANY_IN_BACKGROUND",
    "USE_FULL_SCREEN_INTENT",
    "TURN_SCREEN_ON",
    "MANAGE_ONGOING_CALLS",
    "CHANGE_WIFI_STATE",
    "LOADER_USAGE_STATS",
    "ACCESS_RESTRICTED_SETTINGS",
];

/// Default-app roles shown and restored (ui-spec §3.1 role tags).
pub const ROLES: &[&str] = &["SMS", "DIALER", "BROWSER", "HOME", "ASSISTANT"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AppOp {
    pub op: String,
    /// `allow`, `ignore`, `deny`, `default` or `foreground`.
    pub mode: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SpecialAccess {
    NotificationListener { component: String },
    Accessibility { component: String },
    /// Enabled keyboard; `default` when it is the active one.
    InputMethod { component: String, default: bool },
    /// In the battery-optimization allowlist (`dumpsys deviceidle whitelist +pkg`).
    BatteryUnrestricted,
    DeviceAdmin { component: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub name: String,
    /// e.g. `com.google`.
    pub account_type: String,
}

/// `appops get <pkg>`, keeping only [`SPECIAL_OPS`]. Package lines override the `Uid mode:` ones.
pub fn parse_appops(out: &str) -> Vec<AppOp> {
    let mut ops: Vec<AppOp> = Vec::new();
    for line in out.lines() {
        let line = line.trim();
        let line = line.strip_prefix("Uid mode:").map(str::trim).unwrap_or(line);
        let Some((op, rest)) = line.split_once(": ") else { continue };
        if !SPECIAL_OPS.contains(&op) {
            continue;
        }
        let mode = rest.split(';').next().unwrap_or_default().trim().to_string();
        // `default` means "follow the permission", nothing to restore.
        if mode == "default" {
            ops.retain(|o| o.op != op);
            continue;
        }
        match ops.iter_mut().find(|o| o.op == op) {
            Some(existing) => existing.mode = mode,
            None => ops.push(AppOp { op: op.to_string(), mode }),
        }
    }
    ops
}

/// Special access per package, from `settings get secure`, `dumpsys deviceidle whitelist`
/// and `dumpsys device_policy`.
pub fn parse_special_access(secure: &HashMap<String, String>, deviceidle: &str, device_policy: &str) -> HashMap<String, Vec<SpecialAccess>> {
    let mut map: HashMap<String, Vec<SpecialAccess>> = HashMap::new();
    fn add(map: &mut HashMap<String, Vec<SpecialAccess>>, component: &str, access: SpecialAccess) {
        if let Some((pkg, _)) = component.split_once('/') {
            map.entry(pkg.to_string()).or_default().push(access);
        }
    }
    let list = |key: &str, sep: char| -> Vec<String> {
        secure
            .get(key)
            .filter(|v| v.as_str() != "null")
            .map(|v| v.split(sep).map(str::trim).filter(|s| s.contains('/')).map(str::to_string).collect())
            .unwrap_or_default()
    };
    for c in list("enabled_notification_listeners", ':') {
        add(&mut map, &c, SpecialAccess::NotificationListener { component: c.clone() });
    }
    for c in list("enabled_accessibility_services", ':') {
        add(&mut map, &c, SpecialAccess::Accessibility { component: c.clone() });
    }
    let default_ime = secure.get("default_input_method").cloned().unwrap_or_default();
    // `pkg/.Service;subtype:pkg2/.Other` — subtypes follow `;` within each entry.
    for entry in list("enabled_input_methods", ':') {
        let c = entry.split(';').next().unwrap_or_default().to_string();
        add(&mut map, &c, SpecialAccess::InputMethod { default: c == default_ime, component: c.clone() });
    }
    for line in deviceidle.lines() {
        // Only user additions; `system`/`system-excidle` entries come with the ROM.
        if let Some(rest) = line.trim().strip_prefix("user,") {
            let pkg = rest.split(',').next().unwrap_or_default();
            map.entry(pkg.to_string()).or_default().push(SpecialAccess::BatteryUnrestricted);
        }
    }
    for c in parse_device_admins(device_policy) {
        add(&mut map, &c, SpecialAccess::DeviceAdmin { component: c.clone() });
    }
    map
}

/// Components listed under `Enabled Device Admins (User 0, …):` as `    pkg/.Receiver:`.
fn parse_device_admins(out: &str) -> Vec<String> {
    let mut admins = Vec::new();
    let mut in_section = false;
    for line in out.lines() {
        let indent = line.len() - line.trim_start().len();
        let text = line.trim();
        if text.starts_with("Enabled Device Admins (User 0") {
            in_section = true;
        } else if in_section && indent <= 2 && !text.is_empty() {
            in_section = false;
        } else if in_section && indent == 4 && text.ends_with(':') && text.contains('/') {
            admins.push(text.trim_end_matches(':').to_string());
        }
    }
    admins
}

/// `settings get secure` lines printed as `key=value`.
pub fn parse_key_values(out: &str) -> HashMap<String, String> {
    out.lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect()
}

/// `Account {name=…, type=…}` lines of `dumpsys account` for the first user.
pub fn parse_accounts(out: &str) -> Vec<Account> {
    out.lines()
        .filter_map(|l| {
            let inner = l.trim().strip_prefix("Account {")?.strip_suffix('}')?;
            let (name, ty) = inner.strip_prefix("name=")?.rsplit_once(", type=")?;
            Some(Account { name: name.to_string(), account_type: ty.to_string() })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_special_appops_only() {
        let out = "Uid mode: COARSE_LOCATION: ignore\nRECORD_AUDIO: foreground\nMANAGE_EXTERNAL_STORAGE: allow\n\
                   SYSTEM_ALERT_WINDOW: allow; time=+3h1m ago\nRUN_ANY_IN_BACKGROUND: ignore\nWRITE_SETTINGS: default\n";
        assert_eq!(
            parse_appops(out),
            [
                AppOp { op: "MANAGE_EXTERNAL_STORAGE".into(), mode: "allow".into() },
                AppOp { op: "SYSTEM_ALERT_WINDOW".into(), mode: "allow".into() },
                AppOp { op: "RUN_ANY_IN_BACKGROUND".into(), mode: "ignore".into() },
            ]
        );
    }

    #[test]
    fn special_access_from_pixel_values() {
        let secure = parse_key_values(
            "enabled_notification_listeners=com.google.android.apps.nexuslauncher/com.android.launcher3.notification.NotificationListener:com.google.android.as/com.google.Svc\n\
             enabled_accessibility_services=null\n\
             default_input_method=com.google.android.inputmethod.latin/com.android.inputmethod.latin.LatinIME\n\
             enabled_input_methods=com.google.android.inputmethod.latin/com.android.inputmethod.latin.LatinIME;-858650562:com.google.android.tts/com.google.Voice\n",
        );
        let idle = "system-excidle,com.android.vending,10148\nsystem,com.android.vending,10148\nuser,com.termux,10328\n";
        let dpm = "  Enabled Device Admins (User 0, provisioningState: 0):\n    com.example.mdm/.AdminReceiver:\n      uid=10400\n  Default admin policy size limit: -1\n";
        let map = parse_special_access(&secure, idle, dpm);
        assert_eq!(
            map["com.google.android.inputmethod.latin"],
            [SpecialAccess::InputMethod {
                component: "com.google.android.inputmethod.latin/com.android.inputmethod.latin.LatinIME".into(),
                default: true
            }]
        );
        assert!(matches!(&map["com.google.android.tts"][0], SpecialAccess::InputMethod { default: false, .. }));
        assert!(matches!(&map["com.google.android.as"][0], SpecialAccess::NotificationListener { .. }));
        assert_eq!(map["com.termux"], [SpecialAccess::BatteryUnrestricted]);
        assert!(!map.contains_key("com.android.vending"));
        assert_eq!(
            map["com.example.mdm"],
            [SpecialAccess::DeviceAdmin { component: "com.example.mdm/.AdminReceiver".into() }]
        );
    }

    #[test]
    fn parses_accounts() {
        let out = "User UserInfo{0:Owner:c13}:\n  Accounts: 2\n    Account {name=someone@gmail.com, type=com.google}\n    Account {name=WhatsApp, type=com.whatsapp}\n";
        assert_eq!(
            parse_accounts(out),
            [
                Account { name: "someone@gmail.com".into(), account_type: "com.google".into() },
                Account { name: "WhatsApp".into(), account_type: "com.whatsapp".into() },
            ]
        );
    }
}
