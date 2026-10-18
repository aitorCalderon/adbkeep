//! Restoring the Resto tab (ui-spec §4.2, backup-android.md points 4–16).

use std::path::Path;
use std::sync::atomic::AtomicBool;

use serde::{Deserialize, Serialize};

use super::OtherItem;
use super::wifi::{self, WifiNetwork};
use crate::compat::CompatStatus;
use crate::device::Device;
use crate::format::{Manifest, rel};
use crate::helper::{Helper, InsertSummary};
use crate::restore::REMOTE_TMP;
use crate::root::shell_quote;
use crate::stream::file_to_device;
use crate::{Error, Result};

/// Columns written back to each provider: the portable ones (no IDs, no sync state).
pub const SMS_COLUMNS: &str = "address,body,date,date_sent,type,read,seen,status,subject,service_center,locked,error_code,protocol,reply_path_present";
pub const CALL_COLUMNS: &str = "number,date,duration,type,new,presentation,features,countryiso,name,numbertype,numberlabel,via_number";
pub const BLOCKED_COLUMNS: &str = "original_number,e164_number";
pub const DICTIONARY_COLUMNS: &str = "word,frequency,locale,appid,shortcut";
pub const RAW_CONTACT_COLUMNS: &str = "starred,custom_ringtone,send_to_voicemail,pinned";
pub const CONTACT_DATA_COLUMNS: &str =
    "mimetype,is_primary,is_super_primary,data1,data2,data3,data4,data5,data6,data7,data8,data9,data10,data11,data12,data13,data14,data15";
pub const CALENDAR_COLUMNS: &str = "name,calendar_displayName,calendar_color,calendar_access_level,ownerAccount,visible,sync_events,calendar_timezone,allowedReminders,allowedAvailability,allowedAttendeeTypes";
pub const EVENT_COLUMNS: &str = "title,description,eventLocation,eventColor,eventStatus,dtstart,dtend,duration,eventTimezone,eventEndTimezone,allDay,rrule,rdate,exrule,exdate,hasAlarm,availability,accessLevel,organizer,guestsCanModify,guestsCanInviteOthers,guestsCanSeeGuests";
pub const REMINDER_COLUMNS: &str = "minutes,method";
pub const ATTENDEE_COLUMNS: &str = "attendeeName,attendeeEmail,attendeeRelationship,attendeeType,attendeeStatus";

const MEDIA: &str = "/data/media/0";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OtherRestoreSelection {
    pub sms: bool,
    pub contacts: bool,
    /// Copy the .vcf to the phone and open it in the Contacts app instead of inserting.
    pub contacts_as_vcf: bool,
    pub calls: bool,
    pub blocked: bool,
    pub calendar: bool,
    pub dictionary: bool,
    pub wifi: bool,
    /// "table/key" settings to write.
    pub settings: Vec<String>,
    pub sounds: bool,
    pub wallpaper: bool,
    pub certificates: bool,
    pub folders: Vec<String>,
}

impl OtherRestoreSelection {
    /// In the order they run: system first, files before the sounds that point at them.
    pub fn items(&self) -> Vec<OtherItem> {
        let mut v = Vec::new();
        let mut add = |on: bool, item| {
            if on {
                v.push(item);
            }
        };
        add(self.wifi, OtherItem::Wifi);
        add(!self.settings.is_empty(), OtherItem::Settings);
        add(self.certificates, OtherItem::Certificates);
        add(!self.folders.is_empty(), OtherItem::Files);
        add(self.sounds, OtherItem::Sounds);
        add(self.contacts, OtherItem::Contacts);
        add(self.calls, OtherItem::Calls);
        add(self.sms, OtherItem::Sms);
        add(self.blocked, OtherItem::Blocked);
        add(self.calendar, OtherItem::Calendar);
        add(self.dictionary, OtherItem::Dictionary);
        add(self.wallpaper, OtherItem::Wallpaper);
        v
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum OtherReason {
    /// ⛔ The backup has nothing of this item.
    Empty,
    /// ⛔ Read and written through the helper, which is off.
    HelperRequired,
    /// ⛔ Not restored yet; the backup keeps it for reference.
    NotSupported,
    /// ⚠️ The backup has MMS, which are not restored yet; only the SMS are.
    MmsNotRestored,
    /// ⛔ `cmd wifi add-network` needs Android 11.
    WifiNeedsAndroid11,
    /// ⚠️ Enterprise networks (and some others) cannot be recreated and are skipped.
    WifiSkipped { count: u32 },
    /// ⚠️ The image is copied to the phone; setting it as wallpaper is done by hand.
    WallpaperManual,
    /// ⛔ The backup has no wallpaper image (a built-in or live wallpaper was in use).
    NoWallpaperImage,
    /// ⚠️ Without Android 11 the new files appear after restarting the phone.
    ScanAfterRestart,
}

impl OtherReason {
    fn blocks(&self) -> bool {
        matches!(
            self,
            OtherReason::Empty
                | OtherReason::HelperRequired
                | OtherReason::NotSupported
                | OtherReason::WifiNeedsAndroid11
                | OtherReason::NoWallpaperImage
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OtherCompat {
    pub item: OtherItem,
    pub status: CompatStatus,
    pub reasons: Vec<OtherReason>,
    pub count: Option<u64>,
}

/// What each Resto item of the backup can do on the target. `wifi` is the parsed config, if any.
pub fn preview(manifest: &Manifest, target_sdk: u32, helper: bool, wifi: &[WifiNetwork]) -> Vec<OtherCompat> {
    manifest
        .other
        .iter()
        .map(|entry| {
            let mut reasons = Vec::new();
            if entry.files.is_empty() || entry.count == Some(0) {
                return OtherCompat { item: entry.item, status: CompatStatus::Blocked, reasons: vec![OtherReason::Empty], count: entry.count };
            }
            if entry.item.needs_helper() && !helper {
                reasons.push(OtherReason::HelperRequired);
            }
            match entry.item {
                // The MMS addresses file is only written when there were MMS.
                OtherItem::Sms if entry.files.iter().any(|f| f.ends_with("mms_addr.jsonl")) => {
                    reasons.push(OtherReason::MmsNotRestored)
                }
                OtherItem::Notifications => reasons.push(OtherReason::NotSupported),
                OtherItem::Wifi => {
                    if target_sdk < 30 {
                        reasons.push(OtherReason::WifiNeedsAndroid11);
                    }
                    let skipped = wifi.iter().filter(|n| n.add_network_args().is_none()).count() as u32;
                    if skipped > 0 {
                        reasons.push(OtherReason::WifiSkipped { count: skipped });
                    }
                }
                OtherItem::Wallpaper => {
                    if wallpaper_image(entry).is_some() {
                        reasons.push(OtherReason::WallpaperManual);
                    } else {
                        reasons.push(OtherReason::NoWallpaperImage);
                    }
                }
                OtherItem::Files | OtherItem::Sounds if target_sdk < 30 => reasons.push(OtherReason::ScanAfterRestart),
                _ => {}
            }
            let status = if reasons.iter().any(OtherReason::blocks) {
                CompatStatus::Blocked
            } else if reasons.is_empty() {
                CompatStatus::Ok
            } else {
                CompatStatus::Limited
            };
            OtherCompat { item: entry.item, status, reasons, count: entry.count }
        })
        .collect()
}

fn wallpaper_image(entry: &super::backup::OtherEntry) -> Option<&String> {
    let names = ["wallpaper_orig", "wallpaper", "wallpaper_lock_orig", "wallpaper_lock"];
    names.iter().find_map(|n| entry.files.iter().find(|f| f.rsplit('/').next() == Some(*n)))
}

/// Outcome of one item.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OtherOutcome {
    pub item: OtherItem,
    pub done: u32,
    pub failed: u32,
    pub errors: Vec<String>,
}

impl OtherOutcome {
    fn new(item: OtherItem) -> Self {
        Self { item, done: 0, failed: 0, errors: vec![] }
    }

    fn add(&mut self, s: InsertSummary) {
        self.done += s.inserted;
        self.failed += s.failed;
        self.errors.extend(s.errors);
    }

    fn fail(&mut self, e: impl ToString) {
        self.failed += 1;
        if self.errors.len() < 5 {
            self.errors.push(e.to_string());
        }
    }
}

/// Human-readable commands for "Sólo simular".
pub fn describe(item: OtherItem, sel: &OtherRestoreSelection, wifi: &[WifiNetwork]) -> Vec<String> {
    let h = |args: &str| format!("# helper: {args}");
    match item {
        OtherItem::Sms => vec![h(&format!("insert content://sms other/sms/sms.jsonl {SMS_COLUMNS}"))],
        OtherItem::Contacts if sel.contacts_as_vcf => vec![
            "adb push other/contacts/contacts.vcf /sdcard/Download/adbkeep-contacts.vcf".into(),
            "adb shell am start -a android.intent.action.VIEW -d file:///sdcard/Download/adbkeep-contacts.vcf -t text/x-vcard".into(),
        ],
        OtherItem::Contacts => vec![h("insert-contacts other/contacts/raw_contacts.jsonl other/contacts/data.jsonl")],
        OtherItem::Calls => vec![h(&format!("insert content://call_log/calls other/calls/calls.jsonl {CALL_COLUMNS}"))],
        OtherItem::Blocked => vec![h("insert content://com.android.blockednumber/blocked other/blocked/blocked.jsonl")],
        OtherItem::Calendar => vec![h("insert-calendar other/calendar/{calendars,events,reminders,attendees}.jsonl")],
        OtherItem::Dictionary => vec![h("insert content://user_dictionary/words other/dictionary/dictionary.jsonl")],
        OtherItem::Wifi => wifi
            .iter()
            .filter_map(|n| n.add_network_args())
            .map(|mut a| {
                // Never show passwords, even when simulating.
                if a.len() > 2 && a[2] != "-h" {
                    a[2] = "********".into();
                }
                format!("adb shell cmd wifi add-network {}", a.iter().map(|x| shell_quote(x)).collect::<Vec<_>>().join(" "))
            })
            .collect(),
        OtherItem::Settings => sel
            .settings
            .iter()
            .filter_map(|k| k.split_once('/'))
            .map(|(t, k)| format!("adb shell settings put {t} {k} <value>"))
            .collect(),
        OtherItem::Files => sel.folders.iter().map(|f| format!("tar -xf other/files/{}.tar.zst -C {MEDIA}; chown; restorecon", if f.is_empty() { "_root" } else { f })).collect(),
        OtherItem::Sounds => vec![format!("tar -xf other/sounds.tar.zst -C {MEDIA}; settings put system ringtone …")],
        OtherItem::Certificates => vec!["tar -xf other/certificates.tar.zst -C /data/misc/user/0; chown system; restorecon".into()],
        OtherItem::Wallpaper => vec!["adb push <wallpaper> /sdcard/Pictures/".into()],
        OtherItem::Notifications => vec![],
    }
}

pub struct OtherRestoreJob<'a> {
    pub device: &'a Device,
    pub sdk: u32,
    /// The (unpacked) backup folder.
    pub dir: &'a Path,
    pub manifest: &'a Manifest,
    pub cancel: &'a AtomicBool,
    pub helper: Option<&'a Helper<'a>>,
}

impl OtherRestoreJob<'_> {
    pub async fn run(&self, item: OtherItem, sel: &OtherRestoreSelection) -> Result<OtherOutcome> {
        let mut out = OtherOutcome::new(item);
        let files: Vec<String> = self.manifest.other.iter().find(|e| e.item == item).map(|e| e.files.clone()).unwrap_or_default();
        if item.needs_helper() && self.helper.is_none() && !(item == OtherItem::Contacts && sel.contacts_as_vcf) {
            return Err(Error::HelperRequired);
        }
        match item {
            OtherItem::Sms => {
                let f = self.push("other/sms/sms.jsonl").await?;
                // The SMS provider silently drops writes from apps without WRITE_SMS (default mode: ignore).
                let _ = self.device.root_shell("appops set --uid 0 WRITE_SMS allow").await;
                let res = self.helper()?.insert(&["insert", "content://sms", &f, SMS_COLUMNS]).await;
                let _ = self.device.root_shell("appops set --uid 0 WRITE_SMS default").await;
                out.add(res?);
            }
            OtherItem::Calls => {
                let f = self.push("other/calls/calls.jsonl").await?;
                out.add(self.helper()?.insert(&["insert", "content://call_log/calls", &f, CALL_COLUMNS]).await?);
            }
            OtherItem::Blocked => {
                let f = self.push("other/blocked/blocked.jsonl").await?;
                out.add(self.helper()?.insert(&["insert", "content://com.android.blockednumber/blocked", &f, BLOCKED_COLUMNS]).await?);
            }
            OtherItem::Dictionary => {
                let f = self.push("other/dictionary/dictionary.jsonl").await?;
                out.add(self.helper()?.insert(&["insert", "content://user_dictionary/words", &f, DICTIONARY_COLUMNS]).await?);
            }
            OtherItem::Contacts if sel.contacts_as_vcf => {
                let remote = "/sdcard/Download/adbkeep-contacts.vcf";
                let never = AtomicBool::new(false);
                file_to_device(self.device, &self.dir.join("other/contacts/contacts.vcf"), false, remote, &never, |_| {}).await?;
                self.device
                    .shell(&format!("am start -a android.intent.action.VIEW -d file://{remote} -t text/x-vcard"))
                    .await?
                    .ok()?;
                out.done = 1;
            }
            OtherItem::Contacts => {
                let raw = self.push("other/contacts/raw_contacts.jsonl").await?;
                let data = self.push("other/contacts/data.jsonl").await?;
                out.add(self.helper()?.insert(&["insert-contacts", &raw, &data, RAW_CONTACT_COLUMNS, CONTACT_DATA_COLUMNS]).await?);
            }
            OtherItem::Calendar => {
                let mut remote = Vec::new();
                for t in ["calendars", "events", "reminders", "attendees"] {
                    let rel = format!("other/calendar/{t}.jsonl");
                    remote.push(if files.contains(&rel) { self.push(&rel).await? } else { format!("{REMOTE_TMP}/none.jsonl") });
                }
                let args = [
                    "insert-calendar",
                    &remote[0],
                    &remote[1],
                    &remote[2],
                    &remote[3],
                    CALENDAR_COLUMNS,
                    EVENT_COLUMNS,
                    REMINDER_COLUMNS,
                    ATTENDEE_COLUMNS,
                ];
                out.add(self.helper()?.insert(&args).await?);
            }
            OtherItem::Wifi => {
                if self.sdk < 30 {
                    return Err(Error::NotSupported);
                }
                let xml = std::fs::read_to_string(self.dir.join("other/wifi/WifiConfigStore.xml"))?;
                for net in wifi::parse(&xml) {
                    let Some(args) = net.add_network_args() else {
                        out.fail(format!("{}: not supported", net.ssid));
                        continue;
                    };
                    let quoted: Vec<String> = args.iter().map(|a| shell_quote(a)).collect();
                    match self.device.shell(&format!("cmd wifi add-network {}", quoted.join(" "))).await.and_then(|o| o.ok()) {
                        Ok(_) => out.done += 1,
                        Err(_) => out.fail(net.ssid.clone()),
                    }
                }
            }
            OtherItem::Settings => {
                #[derive(Deserialize)]
                struct Saved {
                    entries: Vec<super::SettingEntry>,
                }
                let saved: Saved = serde_json::from_slice(&std::fs::read(self.dir.join("other/settings.json"))?)
                    .map_err(|e| Error::UnexpectedOutput { what: format!("settings.json: {e}") })?;
                for e in saved.entries.iter().filter(|e| sel.settings.contains(&format!("{}/{}", e.table, e.key))) {
                    if e.value == "null" || !["system", "secure", "global"].contains(&e.table.as_str()) {
                        continue;
                    }
                    let cmd = format!("settings put {} {} {}", e.table, shell_quote(&e.key), shell_quote(&e.value));
                    match self.device.shell(&cmd).await.and_then(|o| o.ok()) {
                        Ok(_) => out.done += 1,
                        Err(_) => out.fail(format!("{}/{}", e.table, e.key)),
                    }
                }
            }
            OtherItem::Files => {
                for folder in &sel.folders {
                    let name = if folder.is_empty() { "_root" } else { folder };
                    match self.extract_media_from(&rel(&["other", "files", &format!("{name}.tar.zst")]), folder).await {
                        Ok(()) => out.done += 1,
                        Err(e) => out.fail(format!("{name}: {e}")),
                    }
                }
                self.scan().await;
            }
            OtherItem::Sounds => {
                for folder in ["Ringtones", "Notifications", "Alarms"] {
                    if let Err(e) = self.extract_media_from("other/sounds.tar.zst", folder).await {
                        out.fail(e);
                        break;
                    }
                }
                self.scan().await;
                self.set_tones(&mut out).await;
            }
            OtherItem::Certificates => {
                let tar = self.push_tar("other/certificates.tar.zst").await?;
                let d = "/data/misc/user/0/cacerts-added";
                self.device
                    .root_shell(&format!(
                        "set -e; mkdir -p /data/misc/user/0; tar -xf {tar} -C /data/misc/user/0; chown -R system:system {d}; \
                         chmod 755 {d}; chmod 644 {d}/*; restorecon -RF {d}"
                    ))
                    .await?
                    .ok()?;
                out.done = self.device.root_shell(&format!("ls {d} | wc -l")).await?.stdout_str().trim().parse().unwrap_or(0);
            }
            OtherItem::Wallpaper => {
                let entry = self.manifest.other.iter().find(|e| e.item == item).ok_or(Error::NotSupported)?;
                let image = wallpaper_image(entry).ok_or(Error::NotSupported)?;
                let never = AtomicBool::new(false);
                let remote = "/sdcard/Pictures/adbkeep-wallpaper.png";
                file_to_device(self.device, &self.dir.join(image), false, remote, &never, |_| {}).await?;
                out.done = 1;
            }
            OtherItem::Notifications => return Err(Error::NotSupported),
        }
        Ok(out)
    }

    fn helper(&self) -> Result<&Helper<'_>> {
        self.helper.ok_or(Error::HelperRequired)
    }

    /// Copies a backup file to the phone's scratch folder; returns its path there.
    async fn push(&self, rel_path: &str) -> Result<String> {
        let remote = format!("{REMOTE_TMP}/{}", rel_path.replace('/', "_"));
        file_to_device(self.device, &self.dir.join(rel_path), false, &remote, self.cancel, |_| {}).await?;
        Ok(remote)
    }

    /// Like [`Self::push`] for a `.tar.zst`, decompressed on the way.
    async fn push_tar(&self, rel_path: &str) -> Result<String> {
        let remote = format!("{REMOTE_TMP}/{}.tar", rel_path.replace('/', "_"));
        file_to_device(self.device, &self.dir.join(rel_path), true, &remote, self.cancel, |_| {}).await?;
        Ok(remote)
    }

    /// Extracts a shared-storage archive into /data/media/0 and gives `folder` to MediaProvider
    /// with the media_rw group (point 11). "" = loose files at the top.
    async fn extract_media_from(&self, archive: &str, folder: &str) -> Result<()> {
        let tar = self.push_tar(archive).await?;
        let owner = self.media_provider_uid().await?;
        let target = if folder.is_empty() {
            // Only the files that came in the archive.
            format!("$(tar -tf {tar} | sed 's|^\\./||' | while read f; do echo {MEDIA}/\"$f\"; done)")
        } else {
            shell_quote(&format!("{MEDIA}/{folder}"))
        };
        let only = if folder.is_empty() { String::new() } else { format!(" {}", shell_quote(folder)) };
        self.device
            .root_shell(&format!(
                "set -e; tar -xf {tar} -C {MEDIA}{only}; for p in {target}; do [ -e \"$p\" ] && chown -R {owner}:1023 \"$p\" && restorecon -RF \"$p\"; done; rm -f {tar}"
            ))
            .await?
            .ok()?;
        Ok(())
    }

    async fn media_provider_uid(&self) -> Result<u32> {
        let out = self.device.shell("pm list packages -U providers.media.module").await?.stdout_str();
        crate::analysis::packages::parse_pm_list_uids(&out)
            .into_iter()
            .map(|(_, uid)| uid)
            .next()
            .ok_or_else(|| Error::UnexpectedOutput { what: "MediaProvider uid".into() })
    }

    /// Tells MediaStore about new files (Android 11+); older versions pick them up on restart.
    async fn scan(&self) {
        if self.sdk >= 30 {
            let _ = self.device.shell("content call --uri content://media/ --method scan_volume --arg external_primary").await;
        }
    }

    /// Re-points ringtone/notification/alarm settings at the restored sounds, found again by
    /// title since MediaStore IDs differ (point 10).
    async fn set_tones(&self, out: &mut OtherOutcome) {
        let (Some(helper), Ok(bytes)) = (self.helper, std::fs::read(self.dir.join("other/sounds.json"))) else { return };
        let Ok(saved) = serde_json::from_slice::<Vec<super::SettingEntry>>(&bytes) else { return };
        for s in saved {
            let Some((volume, title)) = tone_title(&s.value) else { continue };
            let uri = format!("content://media/{volume}/audio/media");
            let found = helper.query(&uri, Some(&["_id"]), Some(&format!("title = {}", sql_string(&title))), None).await;
            match found.ok().and_then(|rows| rows.first().and_then(|r| r.get("_id")).and_then(|v| v.as_u64())) {
                Some(id) => {
                    let value = format!("{uri}/{id}?title={}&canonical=1", url_encode(&title));
                    let cmd = format!("settings put system {} {}", shell_quote(&s.key), shell_quote(&value));
                    if self.device.shell(&cmd).await.and_then(|o| o.ok()).is_ok() {
                        out.done += 1;
                    }
                }
                None => out.fail(format!("{}: {title}", s.key)),
            }
        }
    }
}

/// `content://media/<volume>/audio/media/<id>?title=<t>…` → (volume, decoded title).
pub fn tone_title(uri: &str) -> Option<(String, String)> {
    let rest = uri.strip_prefix("content://media/")?;
    let volume = rest.split('/').next()?.to_string();
    let query = uri.split_once('?')?.1;
    let title = query.split('&').find_map(|p| p.strip_prefix("title="))?;
    Some((volume, url_decode(title)))
}

fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = b.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok());
        match (b[i], hex) {
            (b'%', Some(v)) => {
                out.push(v);
                i += 3;
                continue;
            }
            (b'+', _) => out.push(b' '),
            (c, _) => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|c| if c.is_ascii_alphanumeric() || b"-_.~".contains(&c) { (c as char).to_string() } else { format!("%{c:02X}") })
        .collect()
}

fn sql_string(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Settings marked for restoring when the backup was made ("table/key").
pub fn saved_settings(backup_path: &Path, manifest: &Manifest) -> Vec<String> {
    #[derive(Deserialize)]
    struct Saved {
        selected: Vec<String>,
    }
    if !manifest.files.iter().any(|f| f.path == "other/settings.json") {
        return vec![];
    }
    crate::library::read_file(backup_path, "other/settings.json", 16 * 1024 * 1024)
        .ok()
        .and_then(|b| serde_json::from_slice::<Saved>(&b).ok())
        .map(|s| s.selected)
        .unwrap_or_default()
}

/// Shared-storage folders in the backup ("" = loose files).
pub fn saved_folders(manifest: &Manifest) -> Vec<String> {
    manifest
        .other
        .iter()
        .filter(|e| e.item == OtherItem::Files)
        .flat_map(|e| e.files.iter())
        .filter_map(|f| f.strip_prefix("other/files/")?.strip_suffix(".tar.zst"))
        .map(|n| if n == "_root" { String::new() } else { n.to_string() })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::other::backup::OtherEntry;

    #[test]
    fn tone_titles() {
        assert_eq!(
            tone_title("content://media/internal/audio/media/67?title=Fresh%20Morning&canonical=1"),
            Some(("internal".into(), "Fresh Morning".into()))
        );
        assert_eq!(tone_title("content://settings/system/ringtone"), None);
        assert_eq!(url_encode("Fresh Morning"), "Fresh%20Morning");
        assert_eq!(url_decode("caf%C3%A9%2"), "café%2");
    }

    fn manifest(other: Vec<OtherEntry>) -> Manifest {
        let mut m: Manifest = serde_json::from_str(
            r#"{"formatVersion":1,"appVersion":"","createdAt":"","name":"","source":{"serial":"","manufacturer":"","model":"","androidRelease":"","sdk":36,"abis":[],"buildType":"","buildId":"","rootMethod":null},"apps":[],"accounts":[],"keystorePackages":[],"files":[],"errors":[],"complete":true}"#,
        )
        .unwrap();
        m.other = other;
        m
    }

    fn entry(item: OtherItem, files: &[&str]) -> OtherEntry {
        OtherEntry { item, files: files.iter().map(|s| s.to_string()).collect(), count: Some(1) }
    }

    #[test]
    fn previews_items() {
        let m = manifest(vec![
            entry(OtherItem::Contacts, &["other/contacts/raw_contacts.jsonl"]),
            entry(OtherItem::Wifi, &["other/wifi/WifiConfigStore.xml"]),
            entry(OtherItem::Notifications, &["other/notifications/notification_policy.xml"]),
            entry(OtherItem::Wallpaper, &["other/wallpaper/wallpaper_info.xml"]),
            entry(OtherItem::Sms, &["other/sms/sms.jsonl", "other/sms/mms_addr.jsonl"]),
            OtherEntry { item: OtherItem::Certificates, files: vec![], count: Some(0) },
        ]);
        let p = preview(&m, 36, true, &[]);
        let status: Vec<(OtherItem, CompatStatus)> = p.iter().map(|c| (c.item, c.status)).collect();
        assert_eq!(
            status,
            [
                (OtherItem::Contacts, CompatStatus::Ok),
                (OtherItem::Wifi, CompatStatus::Ok),
                (OtherItem::Notifications, CompatStatus::Blocked),
                (OtherItem::Wallpaper, CompatStatus::Blocked),
                (OtherItem::Sms, CompatStatus::Limited),
                (OtherItem::Certificates, CompatStatus::Blocked),
            ]
        );
        assert_eq!(p[5].reasons, [OtherReason::Empty]);
        let p = preview(&m, 29, false, &[]);
        assert_eq!(p[0].reasons, [OtherReason::HelperRequired]);
        assert_eq!(p[1].reasons, [OtherReason::WifiNeedsAndroid11]);
    }

    #[test]
    fn order_runs_files_before_sounds() {
        let sel = OtherRestoreSelection { sounds: true, folders: vec!["DCIM".into()], wifi: true, contacts: true, ..Default::default() };
        assert_eq!(sel.items(), [OtherItem::Wifi, OtherItem::Files, OtherItem::Sounds, OtherItem::Contacts]);
    }

    #[test]
    fn simulation_hides_wifi_passwords() {
        let nets = vec![WifiNetwork { ssid: "Casa".into(), security: wifi::WifiSecurity::Wpa2, password: Some("secreto".into()), hidden: false }];
        let lines = describe(OtherItem::Wifi, &OtherRestoreSelection::default(), &nets);
        assert_eq!(lines, ["adb shell cmd wifi add-network Casa wpa2 '********'"]);
    }
}
