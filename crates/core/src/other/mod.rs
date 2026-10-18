//! The "Resto" tab (ui-spec §3.3): messages, contacts, calls, blocked numbers, calendar, Wi-Fi,
//! settings, notifications, sounds, wallpaper, dictionary, certificates and shared files
//! (backup-android.md points 4–16).

pub mod backup;
pub mod restore;
pub mod wifi;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::analysis::split_sections;
use crate::device::Device;
use crate::root::shell_quote;
use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum OtherItem {
    Sms,
    Contacts,
    Calls,
    Blocked,
    Calendar,
    Wifi,
    Settings,
    Notifications,
    Sounds,
    Wallpaper,
    Dictionary,
    Certificates,
    Files,
}

impl OtherItem {
    /// Read through content providers, which needs the on-device helper.
    pub fn needs_helper(self) -> bool {
        matches!(self, OtherItem::Sms | OtherItem::Contacts | OtherItem::Calls | OtherItem::Blocked | OtherItem::Calendar | OtherItem::Dictionary)
    }
}

/// Settings keys that mean the same on every Android (point 9). The rest is mostly vendor-
/// or hardware-specific and is not restored by default.
pub const SETTINGS_WHITELIST: &[(&str, &str)] = &[
    ("system", "screen_brightness"),
    ("system", "screen_brightness_mode"),
    ("system", "screen_off_timeout"),
    ("system", "font_scale"),
    ("system", "accelerometer_rotation"),
    ("system", "haptic_feedback_enabled"),
    ("system", "sound_effects_enabled"),
    ("system", "dtmf_tone"),
    ("system", "vibrate_when_ringing"),
    ("system", "time_12_24"),
    ("system", "pointer_speed"),
    ("secure", "location_mode"),
    ("secure", "ui_night_mode"),
    ("secure", "long_press_timeout"),
    ("secure", "multi_press_timeout"),
    ("secure", "accessibility_display_magnification_enabled"),
    ("secure", "high_text_contrast_enabled"),
    ("secure", "accessibility_display_inversion_enabled"),
    ("secure", "accessibility_captioning_enabled"),
    ("secure", "show_ime_with_hard_keyboard"),
    ("secure", "spell_checker_enabled"),
    ("secure", "screensaver_enabled"),
    ("secure", "charging_sounds_enabled"),
    ("global", "auto_time"),
    ("global", "auto_time_zone"),
    ("global", "window_animation_scale"),
    ("global", "transition_animation_scale"),
    ("global", "animator_duration_scale"),
    ("global", "stay_on_while_plugged_in"),
];

pub fn whitelisted(table: &str, key: &str) -> bool {
    SETTINGS_WHITELIST.iter().any(|(t, k)| *t == table && *k == key)
}

/// Shared-storage folders checked by default (point 11); dot folders are MediaProvider caches.
pub fn folder_checked_by_default(name: &str) -> bool {
    !name.starts_with('.') && name != "Android"
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ContactAccount {
    /// "" for contacts stored on the phone without an account.
    pub account_type: String,
    pub count: u32,
    /// Phone/SIM contacts: ticked by default. Accounts and app sync adapters (Google,
    /// WhatsApp…) recreate their own contacts, so they start unticked.
    pub local: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LocalCalendar {
    pub id: u32,
    pub name: String,
    pub events: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    /// Name in /data/media/0; "" stands for the loose files at the top.
    pub name: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SettingEntry {
    pub table: String,
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DbLocation {
    pub name: String,
    pub path: String,
    pub size: u64,
}

/// What the Resto tab shows before backing up: counts and sizes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OtherAnalysis {
    pub sms: Option<u32>,
    pub mms: Option<u32>,
    pub threads: Option<u32>,
    pub contacts: Vec<ContactAccount>,
    pub calls: Option<u32>,
    pub blocked: Option<u32>,
    pub calendars: Vec<LocalCalendar>,
    pub dictionary: Option<u32>,
    pub wifi_networks: Option<u32>,
    pub wifi_path: Option<String>,
    pub settings: Vec<SettingEntry>,
    pub notification_policy: bool,
    /// Ringtones, Notifications and Alarms folders, bytes.
    pub sounds_size: u64,
    pub wallpaper_files: Vec<String>,
    pub certificates: u32,
    pub folders: Vec<Folder>,
    /// Provider databases found (advanced): which copy, CE or DE, has data.
    pub databases: Vec<DbLocation>,
}

const MEDIA: &str = "/data/media/0";
const SYSTEM_USER: &str = "/data/system/users/0";

pub fn wifi_dir(sdk: u32) -> &'static str {
    if sdk >= 30 { "/data/misc/apexdata/com.android.wifi" } else { "/data/misc/wifi" }
}

/// Provider queries behind the counts: (section, uri, projection, selection). A projection of
/// `None` only counts rows.
const PROVIDER_QUERIES: &[(&str, &str, Option<&str>, Option<&str>)] = &[
    ("sms", "content://sms", None, None),
    ("mms", "content://mms", None, None),
    ("threads", "content://mms-sms/conversations?simple=true", None, None),
    ("calls", "content://call_log/calls", None, None),
    ("blocked", "content://com.android.blockednumber/blocked", None, None),
    ("dictionary", "content://user_dictionary/words", None, None),
    ("contacts", "content://com.android.contacts/raw_contacts", Some("account_type"), Some("deleted=0")),
    ("calendars", "content://com.android.calendar/calendars", Some("_id,calendar_displayName"), Some("account_type='LOCAL'")),
    ("events", "content://com.android.calendar/events", Some("calendar_id"), Some("deleted=0")),
];

/// Counts and sizes, reading only. Provider counts come from the helper in one start when it is
/// deployed, else from `content query --projection _id` (enough to count, unlike to extract;
/// about 1.3 s per call).
pub async fn analyze(device: &Device, sdk: u32, helper: Option<&crate::helper::Helper<'_>>) -> Result<OtherAnalysis> {
    let mut s = String::new();
    let mut from_helper = String::new();
    match helper {
        Some(h) => {
            let queries: Vec<crate::helper::BatchQuery> = PROVIDER_QUERIES
                .iter()
                .map(|(_, uri, proj, sel)| crate::helper::BatchQuery { uri, projection: Some(proj.unwrap_or("#count")), selection: *sel })
                .collect();
            // Same text shape as `content query`, so one parser serves both.
            for ((name, _, proj, _), result) in PROVIDER_QUERIES.iter().zip(h.batch(&queries).await?) {
                from_helper += &format!("@@{name}\n");
                let Ok(rows) = result else { continue };
                match proj {
                    None => from_helper += &format!("{}\n", rows.first().and_then(|r| r.get("count")).and_then(|c| c.as_u64()).unwrap_or(0)),
                    Some(p) => {
                        for row in rows {
                            let cols: Vec<String> = p
                                .split(',')
                                .map(|c| {
                                    let v = match row.get(c) {
                                        Some(serde_json::Value::String(s)) => s.clone(),
                                        Some(serde_json::Value::Null) | None => "NULL".into(),
                                        Some(v) => v.to_string(),
                                    };
                                    format!("{c}={v}")
                                })
                                .collect();
                            from_helper += &format!("Row: 0 {}\n", cols.join(", "));
                        }
                    }
                }
            }
        }
        None => {
            for (name, uri, proj, sel) in PROVIDER_QUERIES {
                let where_ = sel.map(|w| format!(" --where {}", shell_quote(w))).unwrap_or_default();
                match proj {
                    None => s += &format!("echo @@{name}; content query --uri {} --projection _id{where_} 2>/dev/null | grep -c '^Row:'; ", shell_quote(uri)),
                    Some(p) => s += &format!("echo @@{name}; content query --uri {} --projection {}{where_} 2>/dev/null; ", shell_quote(uri), p.replace(',', ":")),
                }
            }
        }
    }
    let wifi = format!("{}/WifiConfigStore.xml", wifi_dir(sdk));
    s += &format!("echo @@wifi; [ -f {wifi} ] && grep -c '<Network>' {wifi}; ");
    s += "echo @@notifications; [ -f /data/system/notification_policy.xml ] && echo yes; ";
    s += &format!("echo @@sounds; du -sk {MEDIA}/Ringtones {MEDIA}/Notifications {MEDIA}/Alarms 2>/dev/null; ");
    s += &format!("echo @@wallpaper; ls {SYSTEM_USER} | grep -E '^wallpaper'; ");
    s += "echo @@certificates; ls /data/misc/user/0/cacerts-added 2>/dev/null | wc -l; ";
    s += &format!("echo @@folders; for f in {MEDIA}/* {MEDIA}/.*; do [ -e \"$f\" ] && echo \"$(du -sk \"$f\" | cut -f1) $(basename \"$f\") $([ -d \"$f\" ] && echo d || echo f)\"; done; ");
    s += "echo @@databases; find /data/data /data/user_de/0 -maxdepth 3 \\( -name mmssms.db -o -name contacts2.db -o -name calllog.db \
          -o -name blockednumbers.db -o -name calendar.db -o -name user_dict.db \\) -exec stat -c '%s %n' {} \\; 2>/dev/null; ";
    let root = device.root_shell(&s).await?.stdout_str();
    let settings = device.shell("for t in system secure global; do echo @@$t; settings list $t; done").await?.stdout_str();
    Ok(parse_analysis(&format!("{from_helper}{root}"), &settings))
}

fn parse_analysis(root: &str, settings: &str) -> OtherAnalysis {
    let sec = split_sections(root);
    let get = |k: &str| sec.get(k).map(String::as_str).unwrap_or_default();
    let num = |k: &str| get(k).trim().parse::<u32>().ok();
    let mut a = OtherAnalysis {
        sms: num("sms"),
        mms: num("mms"),
        threads: num("threads"),
        calls: num("calls"),
        blocked: num("blocked"),
        dictionary: num("dictionary"),
        wifi_networks: num("wifi"),
        notification_policy: get("notifications").trim() == "yes",
        certificates: num("certificates").unwrap_or(0),
        ..Default::default()
    };

    let mut accounts: BTreeMap<String, u32> = BTreeMap::new();
    for line in get("contacts").lines().filter(|l| l.starts_with("Row:")) {
        let ty = line.split_once("account_type=").map(|(_, v)| v.trim()).unwrap_or("NULL");
        let ty = if ty == "NULL" { "" } else { ty };
        *accounts.entry(ty.to_string()).or_default() += 1;
    }
    a.contacts = accounts
        .into_iter()
        .map(|(t, count)| ContactAccount { local: is_local_account(&t), account_type: t, count })
        .collect();

    let mut events: BTreeMap<u32, u32> = BTreeMap::new();
    for line in get("events").lines() {
        if let Some(id) = line.split_once("calendar_id=").and_then(|(_, v)| v.trim().parse().ok()) {
            *events.entry(id).or_default() += 1;
        }
    }
    a.calendars = get("calendars")
        .lines()
        .filter_map(|l| {
            let (head, name) = l.split_once(", calendar_displayName=")?;
            let id: u32 = head.split_once("_id=")?.1.trim().parse().ok()?;
            Some(LocalCalendar { id, name: name.to_string(), events: events.get(&id).copied().unwrap_or(0) })
        })
        .collect();

    a.sounds_size = get("sounds").lines().filter_map(|l| l.split_whitespace().next()?.parse::<u64>().ok()).sum::<u64>() * 1024;
    a.wallpaper_files = get("wallpaper").lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect();
    let mut loose = 0u64;
    for line in get("folders").lines() {
        let mut w = line.splitn(2, ' ');
        let (Some(kb), Some(rest)) = (w.next().and_then(|k| k.parse::<u64>().ok()), w.next()) else { continue };
        let Some((name, kind)) = rest.rsplit_once(' ') else { continue };
        if name == "." || name == ".." {
            continue;
        }
        if kind == "d" {
            a.folders.push(Folder { name: name.to_string(), size: kb * 1024 });
        } else {
            loose += kb * 1024;
        }
    }
    a.folders.sort_by_key(|f| f.name.to_lowercase());
    if loose > 0 {
        a.folders.push(Folder { name: String::new(), size: loose });
    }
    a.databases = get("databases")
        .lines()
        .filter_map(|l| {
            let (size, path) = l.trim().split_once(' ')?;
            Some(DbLocation { name: path.rsplit('/').next()?.to_string(), path: path.to_string(), size: size.parse().ok()? })
        })
        .collect();

    let ssec = split_sections(settings);
    for table in ["system", "secure", "global"] {
        for line in ssec.get(table).map(String::as_str).unwrap_or_default().lines() {
            if let Some((k, v)) = line.split_once('=') {
                a.settings.push(SettingEntry { table: table.into(), key: k.to_string(), value: v.to_string() });
            }
        }
    }
    a
}

/// Contacts kept only on the phone or SIM (as opposed to accounts that sync them back).
pub fn is_local_account(ty: &str) -> bool {
    let t = ty.to_lowercase();
    t.is_empty() || t.contains("sim") || t.contains("local") || t.contains(".phone") || t == "com.android.contacts.default" || t == "com.android.contacts"
}

/// Errors per item during a backup of the Resto tab.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ItemFailure {
    pub item: OtherItem,
    pub error: Error,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_counts_accounts_calendars_and_folders() {
        let root = "@@sms\n0\n@@mms\n0\n@@threads\n0\n@@calls\n12\n@@blocked\n0\n@@dictionary\n3\n\
            @@contacts\nRow: 0 account_type=NULL\nRow: 1 account_type=NULL\nRow: 2 account_type=com.google\nRow: 3 account_type=com.whatsapp\nRow: 4 account_type=vnd.sec.contact.sim\n\
            @@calendars\nRow: 0 _id=1, calendar_displayName=adbkeep prueba, con coma\n\
            @@events\nRow: 0 calendar_id=1\nRow: 1 calendar_id=1\nRow: 2 calendar_id=7\n\
            @@wifi\n1\n@@notifications\nyes\n@@sounds\n4\t/data/media/0/Ringtones\n8\t/data/media/0/Alarms\n\
            @@wallpaper\nwallpaper_info.xml\n@@certificates\n0\n\
            @@folders\n971110 Android d\n4 DCIM d\n1608 Metrics v5.zip f\n4 .transforms d\n4 . d\n\
            @@databases\n118784 /data/data/com.android.providers.telephony/databases/mmssms.db\n";
        let a = parse_analysis(root, "@@system\nfont_scale=1.0\n@@secure\nx=a=b\n@@global\n");
        assert_eq!((a.sms, a.calls, a.dictionary, a.wifi_networks), (Some(0), Some(12), Some(3), Some(1)));
        assert_eq!(
            a.contacts.iter().map(|c| (c.account_type.as_str(), c.count, c.local)).collect::<Vec<_>>(),
            [("", 2, true), ("com.google", 1, false), ("com.whatsapp", 1, false), ("vnd.sec.contact.sim", 1, true)]
        );
        assert_eq!(a.calendars, [LocalCalendar { id: 1, name: "adbkeep prueba, con coma".into(), events: 2 }]);
        assert!(a.notification_policy);
        assert_eq!(a.sounds_size, 12 * 1024);
        assert_eq!(
            a.folders.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
            [".transforms", "Android", "DCIM", ""]
        );
        assert_eq!(a.folders[3].size, 1608 * 1024);
        assert_eq!(a.databases[0].name, "mmssms.db");
        assert_eq!(a.settings[1], SettingEntry { table: "secure".into(), key: "x".into(), value: "a=b".into() });
        assert!(whitelisted("system", "font_scale"));
        assert!(!folder_checked_by_default("Android") && !folder_checked_by_default(".transforms"));
    }
}
