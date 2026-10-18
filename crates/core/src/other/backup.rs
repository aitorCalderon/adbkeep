//! Backing up the Resto tab items into `other/` of the backup folder.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use serde::{Deserialize, Serialize};

use super::{MEDIA, OtherAnalysis, OtherItem, SYSTEM_USER, wifi_dir};
use crate::device::Device;
use crate::format::{FileEntry, ItemError, rel, write_json};
use crate::helper::Helper;
use crate::root::shell_quote;
use crate::stream::{root_to_file, sha256_file};
use crate::{Error, Result};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OtherSelection {
    pub sms: bool,
    /// Contact account types to save ("" = on the phone); empty saves no contacts.
    pub contacts: Vec<String>,
    pub calls: bool,
    pub blocked: bool,
    pub calendar: bool,
    pub wifi: bool,
    /// Settings to restore later, as "table/key". All settings are saved for reference;
    /// `None` saves none.
    pub settings: Option<Vec<String>>,
    pub notifications: bool,
    pub sounds: bool,
    pub wallpaper: bool,
    pub dictionary: bool,
    pub certificates: bool,
    /// Folders of shared storage ("" = loose files at the top).
    pub folders: Vec<String>,
    /// `tar --exclude` patterns for the folders (default `.thumbnails`).
    pub exclude: Vec<String>,
}

impl OtherSelection {
    pub fn items(&self) -> Vec<OtherItem> {
        let mut v = Vec::new();
        let mut add = |on: bool, item| {
            if on {
                v.push(item);
            }
        };
        add(self.sms, OtherItem::Sms);
        add(!self.contacts.is_empty(), OtherItem::Contacts);
        add(self.calls, OtherItem::Calls);
        add(self.blocked, OtherItem::Blocked);
        add(self.calendar, OtherItem::Calendar);
        add(self.wifi, OtherItem::Wifi);
        add(self.settings.is_some(), OtherItem::Settings);
        add(self.notifications, OtherItem::Notifications);
        add(self.sounds, OtherItem::Sounds);
        add(self.wallpaper, OtherItem::Wallpaper);
        add(self.dictionary, OtherItem::Dictionary);
        add(self.certificates, OtherItem::Certificates);
        add(!self.folders.is_empty(), OtherItem::Files);
        v
    }
}

/// What the backup holds for one item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OtherEntry {
    pub item: OtherItem,
    pub files: Vec<String>,
    /// Rows, networks, files… depending on the item.
    pub count: Option<u64>,
}

/// Expected bytes from the phone for an item, for progress.
pub fn estimate(item: OtherItem, sel: &OtherSelection, a: &OtherAnalysis) -> u64 {
    match item {
        OtherItem::Sounds => a.sounds_size,
        OtherItem::Files => a.folders.iter().filter(|f| sel.folders.contains(&f.name)).map(|f| f.size).sum(),
        _ => 64 * 1024,
    }
}

/// Where each item's files go and how; one item at a time.
pub struct OtherJob<'a> {
    pub device: &'a Device,
    pub sdk: u32,
    pub dir: &'a Path,
    pub tmp: &'a Path,
    pub level: i32,
    pub cancel: &'a AtomicBool,
    pub analysis: &'a OtherAnalysis,
    pub helper: Option<&'a Helper<'a>>,
}

impl OtherJob<'_> {
    pub async fn run(&self, item: OtherItem, sel: &OtherSelection, files: &mut Vec<FileEntry>) -> Result<OtherEntry> {
        if item.needs_helper() && self.helper.is_none() {
            return Err(Error::HelperRequired);
        }
        let mut entry = OtherEntry { item, files: vec![], count: None };
        match item {
            OtherItem::Sms => self.sms(&mut entry).await?,
            OtherItem::Contacts => self.contacts(&sel.contacts, &mut entry).await?,
            OtherItem::Calls => entry.count = Some(self.query("calls.jsonl", "content://call_log/calls", None, &mut entry).await?),
            OtherItem::Blocked => {
                entry.count = Some(self.query("blocked.jsonl", "content://com.android.blockednumber/blocked", None, &mut entry).await?)
            }
            OtherItem::Calendar => self.calendar(&mut entry).await?,
            OtherItem::Dictionary => {
                entry.count = Some(self.query("dictionary.jsonl", "content://user_dictionary/words", None, &mut entry).await?)
            }
            OtherItem::Wifi => {
                let dir = wifi_dir(self.sdk);
                for name in ["WifiConfigStore.xml", "WifiConfigStoreSoftAp.xml"] {
                    match self.copy(&format!("{dir}/{name}"), &rel(&["other", "wifi", name])).await {
                        Ok(p) => entry.files.push(p),
                        Err(e) if name == "WifiConfigStore.xml" => return Err(e),
                        Err(_) => {}
                    }
                }
                entry.count = self.analysis.wifi_networks.map(u64::from);
            }
            OtherItem::Settings => {
                #[derive(Serialize)]
                struct Saved<'a> {
                    entries: &'a [super::SettingEntry],
                    selected: &'a [String],
                }
                let path = rel(&["other", "settings.json"]);
                let selected = sel.settings.clone().unwrap_or_default();
                write_json(&self.dir.join(&path), &Saved { entries: &self.analysis.settings, selected: &selected })?;
                entry.files.push(path);
                entry.count = Some(selected.len() as u64);
            }
            OtherItem::Notifications => {
                entry.files.push(self.copy("/data/system/notification_policy.xml", &rel(&["other", "notifications", "notification_policy.xml"])).await?);
            }
            OtherItem::Sounds => {
                let path = rel(&["other", "sounds.tar.zst"]);
                self.tar(&format!("tar -cpf - -C {MEDIA} Ringtones Notifications Alarms"), &path).await?;
                entry.files.push(path);
                // The chosen sounds point at MediaStore IDs; `title=` lets the restore find them again (point 10).
                let uris: Vec<&super::SettingEntry> = self
                    .analysis
                    .settings
                    .iter()
                    .filter(|s| s.table == "system" && ["ringtone", "notification_sound", "alarm_alert"].contains(&s.key.as_str()))
                    .collect();
                let path = rel(&["other", "sounds.json"]);
                write_json(&self.dir.join(&path), &uris)?;
                entry.files.push(path);
            }
            OtherItem::Wallpaper => {
                for name in &self.analysis.wallpaper_files {
                    entry.files.push(self.copy(&format!("{SYSTEM_USER}/{name}"), &rel(&["other", "wallpaper", name])).await?);
                }
            }
            OtherItem::Certificates => {
                if self.analysis.certificates > 0 {
                    let path = rel(&["other", "certificates.tar.zst"]);
                    self.tar("tar -cpf - -C /data/misc/user/0 cacerts-added", &path).await?;
                    entry.files.push(path);
                }
                entry.count = Some(self.analysis.certificates.into());
            }
            OtherItem::Files => {
                let excludes: String = sel.exclude.iter().map(|e| format!(" --exclude={}", shell_quote(e))).collect();
                for folder in &sel.folders {
                    let (cmd, name) = if folder.is_empty() {
                        // Loose files at the top of shared storage.
                        (format!("cd {MEDIA} && find . -maxdepth 1 -type f | tar -cpf -{excludes} -T -"), "_root".to_string())
                    } else {
                        if folder.contains('/') || folder == ".." {
                            continue;
                        }
                        // Sockets make toybox tar fail (see `backup::plan`).
                        let q = shell_quote(folder);
                        (format!("cd {MEDIA} && find {q} -type s | tar -cpf - -X /dev/stdin{excludes} {q}"), folder.clone())
                    };
                    let path = rel(&["other", "files", &format!("{name}.tar.zst")]);
                    self.tar(&cmd, &path).await?;
                    entry.files.push(path);
                }
                entry.count = Some(sel.folders.len() as u64);
            }
        }
        for f in &entry.files {
            let p = self.dir.join(f);
            files.push(FileEntry { path: f.clone(), size: std::fs::metadata(&p)?.len(), sha256: sha256_file(&p)? });
        }
        Ok(entry)
    }

    fn helper(&self) -> &Helper<'_> {
        self.helper.expect("checked in run")
    }

    async fn query(&self, name: &str, uri: &str, selection: Option<&str>, entry: &mut OtherEntry) -> Result<u64> {
        let path = rel(&["other", item_dir(entry.item), name]);
        let dest = self.dir.join(&path);
        std::fs::create_dir_all(dest.parent().unwrap())?;
        let n = self.helper().query_to_file(uri, selection, &dest).await?;
        entry.files.push(path);
        Ok(n)
    }

    /// `column IN (ids)` queries in chunks, appended into one file.
    async fn query_in(&self, name: &str, uri: &str, column: &str, ids: &[String], entry: &mut OtherEntry) -> Result<u64> {
        let path = rel(&["other", item_dir(entry.item), name]);
        let dest = self.dir.join(&path);
        std::fs::create_dir_all(dest.parent().unwrap())?;
        let mut out = std::fs::File::create(&dest)?;
        let part = self.tmp.join(format!("{name}.part"));
        let mut total = 0;
        for chunk in ids.chunks(500) {
            total += self.helper().query_to_file(uri, Some(&format!("{column} IN ({})", chunk.join(","))), &part).await?;
            std::io::copy(&mut std::fs::File::open(&part)?, &mut out)?;
        }
        let _ = std::fs::remove_file(&part);
        entry.files.push(path);
        Ok(total)
    }

    async fn copy(&self, remote: &str, path: &str) -> Result<String> {
        let dest = self.dir.join(path);
        std::fs::create_dir_all(dest.parent().unwrap())?;
        root_to_file(self.device, &format!("cat {}", shell_quote(remote)), &dest, None, self.cancel, |_| {}).await?;
        Ok(path.to_string())
    }

    async fn tar(&self, cmd: &str, path: &str) -> Result<()> {
        let dest = self.dir.join(path);
        std::fs::create_dir_all(dest.parent().unwrap())?;
        root_to_file(self.device, cmd, &dest, Some(self.level), self.cancel, |_| {}).await?;
        Ok(())
    }

    async fn sms(&self, entry: &mut OtherEntry) -> Result<()> {
        let sms = self.query("sms.jsonl", "content://sms", None, entry).await?;
        let mms = self.query("mms.jsonl", "content://mms", None, entry).await?;
        self.query("mms_parts.jsonl", "content://mms/part", None, entry).await?;
        let ids = ids_in(&self.dir.join(rel(&["other", "sms", "mms.jsonl"])), "_id")?;
        if !ids.is_empty() {
            let path = rel(&["other", "sms", "mms_addr.jsonl"]);
            self.helper().query_each_to_file("content://mms/{id}/addr", &ids, &self.dir.join(&path)).await?;
            entry.files.push(path);
            // Part files live next to the database that holds the messages (CE or DE).
            if let Some(db) = self.analysis.databases.iter().filter(|d| d.name == "mmssms.db").max_by_key(|d| d.size) {
                let provider = Path::new(&db.path).parent().and_then(Path::parent).map(PathBuf::from);
                if let Some(p) = provider {
                    let path = rel(&["other", "sms", "app_parts.tar.zst"]);
                    let cmd = format!("[ -d {d}/app_parts ] && tar -cpf - -C {d} app_parts", d = shell_quote(&p.display().to_string()));
                    if self.tar(&cmd, &path).await.is_ok() {
                        entry.files.push(path);
                    }
                }
            }
        }
        entry.count = Some(sms + mms);
        Ok(())
    }

    async fn contacts(&self, types: &[String], entry: &mut OtherEntry) -> Result<()> {
        let mut conds: Vec<String> = Vec::new();
        if types.iter().any(|t| t.is_empty()) {
            conds.push("account_type IS NULL".into());
        }
        let named: Vec<String> = types.iter().filter(|t| !t.is_empty()).map(|t| sql_string(t)).collect();
        if !named.is_empty() {
            conds.push(format!("account_type IN ({})", named.join(",")));
        }
        let selection = format!("deleted=0 AND ({})", conds.join(" OR "));
        let n = self.query("raw_contacts.jsonl", "content://com.android.contacts/raw_contacts", Some(&selection), entry).await?;
        let raw = self.dir.join(rel(&["other", "contacts", "raw_contacts.jsonl"]));
        let raw_ids = ids_in(&raw, "_id")?;
        if raw_ids.is_empty() {
            entry.count = Some(0);
            return Ok(());
        }
        self.query_in("data.jsonl", "content://com.android.contacts/data", "raw_contact_id", &raw_ids, entry).await?;
        // vCard of the aggregate contacts, for opening with any contacts app (point 4).
        let mut contact_ids = ids_in(&raw, "contact_id")?;
        contact_ids.sort();
        contact_ids.dedup();
        let lookups_path = self.tmp.join("lookups.jsonl");
        let mut keys: Vec<String> = Vec::new();
        for chunk in contact_ids.chunks(500) {
            self.helper()
                .query_to_file("content://com.android.contacts/contacts", Some(&format!("_id IN ({})", chunk.join(","))), &lookups_path)
                .await?;
            keys.extend(ids_in(&lookups_path, "lookup")?);
        }
        let _ = std::fs::remove_file(&lookups_path);
        let vcf_path = rel(&["other", "contacts", "contacts.vcf"]);
        let vcf = self.dir.join(&vcf_path);
        let mut out = std::fs::File::create(&vcf)?;
        let part = self.tmp.join("contacts.vcf.part");
        for chunk in keys.chunks(100) {
            let uri = format!("content://com.android.contacts/contacts/as_multi_vcard/{}", chunk.join(":"));
            self.helper().asset_to_file(&uri, &part).await?;
            std::io::copy(&mut std::fs::File::open(&part)?, &mut out)?;
        }
        let _ = std::fs::remove_file(&part);
        entry.files.push(vcf_path);
        entry.count = Some(n);
        Ok(())
    }

    async fn calendar(&self, entry: &mut OtherEntry) -> Result<()> {
        // Only local calendars: account ones (Google…) sync back by themselves (point 7).
        self.query("calendars.jsonl", "content://com.android.calendar/calendars", Some("account_type='LOCAL'"), entry).await?;
        let cal_ids = ids_in(&self.dir.join(rel(&["other", "calendar", "calendars.jsonl"])), "_id")?;
        if cal_ids.is_empty() {
            entry.count = Some(0);
            return Ok(());
        }
        let events = self.query_in("events.jsonl", "content://com.android.calendar/events", "calendar_id", &cal_ids, entry).await?;
        let event_ids = ids_in(&self.dir.join(rel(&["other", "calendar", "events.jsonl"])), "_id")?;
        if !event_ids.is_empty() {
            self.query_in("reminders.jsonl", "content://com.android.calendar/reminders", "event_id", &event_ids, entry).await?;
            self.query_in("attendees.jsonl", "content://com.android.calendar/attendees", "event_id", &event_ids, entry).await?;
        }
        entry.count = Some(events);
        Ok(())
    }
}

fn item_dir(item: OtherItem) -> &'static str {
    match item {
        OtherItem::Sms => "sms",
        OtherItem::Contacts => "contacts",
        OtherItem::Calls => "calls",
        OtherItem::Blocked => "blocked",
        OtherItem::Calendar => "calendar",
        OtherItem::Dictionary => "dictionary",
        _ => "misc",
    }
}

/// Values of `column` (numbers or strings) in a JSON-lines file, as SQL/URI-ready text.
fn ids_in(path: &Path, column: &str) -> Result<Vec<String>> {
    let text = std::fs::read_to_string(path)?;
    Ok(text
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| match v.get(column)? {
            serde_json::Value::Number(n) => Some(n.to_string()),
            serde_json::Value::String(s) => Some(s.clone()),
            _ => None,
        })
        .collect())
}

/// A quoted SQL string literal.
fn sql_string(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Records an item failure the way app failures are recorded.
pub fn failure(item: OtherItem, error: Error) -> ItemError {
    ItemError { package: None, step: format!("other:{}", serde_json::to_value(item).unwrap().as_str().unwrap()), error }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_and_helpers() {
        let sel = OtherSelection { sms: true, contacts: vec!["".into()], wifi: true, folders: vec!["DCIM".into()], ..Default::default() };
        assert_eq!(sel.items(), [OtherItem::Sms, OtherItem::Contacts, OtherItem::Wifi, OtherItem::Files]);
        assert_eq!(sql_string("it's"), "'it''s'");
        let dir = std::env::temp_dir().join(format!("adbkeep-ids-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("x.jsonl");
        std::fs::write(&f, "{\"_id\":3,\"lookup\":\"0r1-ABC\"}\nnoise\n{\"_id\":7,\"lookup\":null}\n").unwrap();
        assert_eq!(ids_in(&f, "_id").unwrap(), ["3", "7"]);
        assert_eq!(ids_in(&f, "lookup").unwrap(), ["0r1-ABC"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
