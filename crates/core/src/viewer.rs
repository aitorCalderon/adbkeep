//! Viewers and exports of "Mis copias" (ui-spec §5): messages, contacts, calls, Wi-Fi, settings
//! and shared files read from a backup on the PC (folder or packed), plus `.vcf`, the XML of
//! *SMS Backup & Restore* (backup-android.md point 6) and loose files. Nothing here talks to a phone.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::library::{read_files, with_file};
use crate::other::wifi::{self, WifiSecurity};
use crate::{Error, Result};

type Row = Map<String, Value>;

/// The JSON-lines files of the Resto tab are read whole; this only guards against nonsense.
const ROWS_LIMIT: usize = 512 * 1024 * 1024;
/// MMS attachments shown in the viewer.
const PART_LIMIT: usize = 32 * 1024 * 1024;

const SMS: &str = "other/sms/sms.jsonl";
const MMS: &str = "other/sms/mms.jsonl";
const MMS_PARTS: &str = "other/sms/mms_parts.jsonl";
const MMS_ADDR: &str = "other/sms/mms_addr.jsonl";
const APP_PARTS: &str = "other/sms/app_parts.tar.zst";
const RAW_CONTACTS: &str = "other/contacts/raw_contacts.jsonl";
const CONTACT_DATA: &str = "other/contacts/data.jsonl";
const CONTACTS_VCF: &str = "other/contacts/contacts.vcf";
const CALLS: &str = "other/calls/calls.jsonl";
const WIFI: &str = "other/wifi/WifiConfigStore.xml";
const SETTINGS: &str = "other/settings.json";

fn rows(files: &HashMap<String, Vec<u8>>, rel: &str) -> Vec<Row> {
    let Some(bytes) = files.get(rel) else { return vec![] };
    String::from_utf8_lossy(bytes).lines().filter_map(|l| serde_json::from_str::<Row>(l).ok()).collect()
}

fn text(r: &Row, k: &str) -> Option<String> {
    match r.get(k)? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn int(r: &Row, k: &str) -> Option<i64> {
    match r.get(k)? {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

/// Blobs come from the helper as `{"base64": "…"}`.
fn blob(r: &Row, k: &str) -> Option<String> {
    r.get(k)?.get("base64")?.as_str().map(str::to_string)
}

// ---------------------------------------------------------------- contacts

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum FieldKind {
    Phone,
    Email,
    Address,
    Organization,
    Website,
    Nickname,
    Event,
    Im,
    Relation,
    Sip,
    Note,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ContactField {
    pub kind: FieldKind,
    pub value: String,
    /// Android's `TYPE_*` code of the field (home, mobile, work…), translated by the UI.
    pub type_code: Option<i64>,
    /// Custom label, when the type is "custom".
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Contact {
    pub id: i64,
    pub name: String,
    /// Account types of the raw contacts merged in it ("" = on the phone).
    pub accounts: Vec<String>,
    pub starred: bool,
    pub fields: Vec<ContactField>,
    /// JPEG thumbnail, base64.
    pub photo: Option<String>,
}

pub fn contacts(path: &Path) -> Result<Vec<Contact>> {
    let files = read_files(path, &[RAW_CONTACTS, CONTACT_DATA], ROWS_LIMIT)?;
    Ok(build_contacts(&rows(&files, RAW_CONTACTS), &rows(&files, CONTACT_DATA)))
}

fn build_contacts(raw: &[Row], data: &[Row]) -> Vec<Contact> {
    // Raw contacts of the same person (one per account) share `contact_id`.
    let mut group_of: HashMap<i64, i64> = HashMap::new();
    let mut contacts: BTreeMap<i64, Contact> = BTreeMap::new();
    for r in raw {
        let Some(id) = int(r, "_id") else { continue };
        let group = int(r, "contact_id").unwrap_or(-id);
        group_of.insert(id, group);
        let c = contacts.entry(group).or_insert_with(|| Contact {
            id: group,
            name: String::new(),
            accounts: vec![],
            starred: false,
            fields: vec![],
            photo: None,
        });
        let account = text(r, "account_type").unwrap_or_default();
        if !c.accounts.contains(&account) {
            c.accounts.push(account);
        }
        c.starred |= int(r, "starred") == Some(1);
        if c.name.is_empty() {
            c.name = text(r, "display_name").unwrap_or_default();
        }
    }
    for d in data {
        let Some(raw_id) = int(d, "raw_contact_id") else { continue };
        let Some(c) = group_of.get(&raw_id).and_then(|g| contacts.get_mut(g)) else { continue };
        let mime = text(d, "mimetype").unwrap_or_default();
        let kind = match mime.trim_start_matches("vnd.android.cursor.item/") {
            "name" => {
                if let Some(n) = text(d, "data1") {
                    c.name = n;
                }
                continue;
            }
            "photo" => {
                if c.photo.is_none() {
                    c.photo = blob(d, "data15");
                }
                continue;
            }
            "phone_v2" => FieldKind::Phone,
            "email_v2" => FieldKind::Email,
            "postal-address_v2" => FieldKind::Address,
            "organization" => FieldKind::Organization,
            "website" => FieldKind::Website,
            "nickname" => FieldKind::Nickname,
            "contact_event" => FieldKind::Event,
            "im" => FieldKind::Im,
            "relation" => FieldKind::Relation,
            "sip_address" => FieldKind::Sip,
            "note" => FieldKind::Note,
            _ => continue,
        };
        let value = match kind {
            FieldKind::Organization => {
                let parts: Vec<String> = [text(d, "data1"), text(d, "data4")].into_iter().flatten().collect();
                parts.join(" · ")
            }
            _ => text(d, "data1").unwrap_or_default(),
        };
        if value.is_empty() || c.fields.iter().any(|f| f.kind == kind && f.value == value) {
            continue;
        }
        let type_code = if kind == FieldKind::Organization { None } else { int(d, "data2") };
        c.fields.push(ContactField { kind, value, type_code, label: text(d, "data3").filter(|_| type_code == Some(0)) });
    }
    let mut out: Vec<Contact> = contacts.into_values().collect();
    for c in &mut out {
        c.fields.sort_by_key(|f| f.kind as u8);
    }
    out.sort_by_cached_key(|c| (c.name.is_empty(), c.name.to_lowercase()));
    out
}

/// Phone number → contact name, for messages and calls. Numbers are compared by their last
/// nine digits, which ignores country prefixes and formatting.
fn names_by_number(contacts: &[Contact]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for c in contacts.iter().filter(|c| !c.name.is_empty()) {
        for f in c.fields.iter().filter(|f| f.kind == FieldKind::Phone) {
            map.entry(number_key(&f.value)).or_insert_with(|| c.name.clone());
        }
    }
    map
}

fn number_key(number: &str) -> String {
    let digits: String = number.chars().filter(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return number.to_lowercase();
    }
    digits[digits.len().saturating_sub(9)..].to_string()
}

/// Writes the contacts as vCard: the one made by the phone when the backup has it, otherwise
/// one built from the rows. Returns the number of cards.
pub fn export_vcf(path: &Path, dest: &Path) -> Result<u32> {
    let files = read_files(path, &[CONTACTS_VCF], ROWS_LIMIT)?;
    let bytes = match files.get(CONTACTS_VCF) {
        Some(b) if !b.is_empty() => b.clone(),
        _ => to_vcard(&contacts(path)?).into_bytes(),
    };
    write_atomic(dest, &bytes)?;
    Ok(String::from_utf8_lossy(&bytes).matches("BEGIN:VCARD").count() as u32)
}

fn to_vcard(contacts: &[Contact]) -> String {
    fn esc(s: &str) -> String {
        s.replace('\\', "\\\\").replace(',', "\\,").replace(';', "\\;").replace('\n', "\\n")
    }
    let mut out = String::new();
    for c in contacts {
        out.push_str("BEGIN:VCARD\r\nVERSION:3.0\r\n");
        out.push_str(&format!("FN:{}\r\nN:{};;;;\r\n", esc(&c.name), esc(&c.name)));
        for f in &c.fields {
            let line = match f.kind {
                FieldKind::Phone => {
                    let ty = match f.type_code {
                        Some(1) => "HOME",
                        Some(2) => "CELL",
                        Some(3) => "WORK",
                        _ => "VOICE",
                    };
                    format!("TEL;TYPE={ty}:{}", esc(&f.value))
                }
                FieldKind::Email => format!("EMAIL;TYPE=INTERNET:{}", esc(&f.value)),
                FieldKind::Address => format!("ADR:;;{};;;;", esc(&f.value)),
                FieldKind::Organization => format!("ORG:{}", esc(&f.value)),
                FieldKind::Website => format!("URL:{}", esc(&f.value)),
                FieldKind::Nickname => format!("NICKNAME:{}", esc(&f.value)),
                FieldKind::Event if f.type_code == Some(3) => format!("BDAY:{}", esc(&f.value)),
                FieldKind::Note => format!("NOTE:{}", esc(&f.value)),
                FieldKind::Sip => format!("IMPP:sip:{}", esc(&f.value)),
                FieldKind::Event | FieldKind::Im | FieldKind::Relation => continue,
            };
            out.push_str(&line);
            out.push_str("\r\n");
        }
        if let Some(photo) = &c.photo {
            out.push_str(&format!("PHOTO;ENCODING=b;TYPE=JPEG:{photo}\r\n"));
        }
        out.push_str("END:VCARD\r\n");
    }
    out
}

// ---------------------------------------------------------------- messages

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    /// `_id` of the MMS part, to load or save it.
    pub part: i64,
    pub mime: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub mms: bool,
    /// Milliseconds since the epoch.
    pub date: i64,
    pub outgoing: bool,
    /// Sender in group MMS.
    pub from: Option<String>,
    pub body: String,
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub thread: i64,
    pub addresses: Vec<String>,
    /// Names from the contacts in the same backup, by address.
    pub names: Vec<Option<String>>,
    pub last_date: i64,
    /// Oldest first.
    pub messages: Vec<Message>,
}

const ADDR_FROM: i64 = 137;
const ADDR_TO: i64 = 151;
/// Placeholder Android puts in the FROM row of sent MMS.
const ADDR_TOKEN: &str = "insert-address-token";

/// SMS: 1 inbox; 2 sent, 3 draft, 4 outbox, 5 failed, 6 queued. MMS `msg_box`: 1 inbox;
/// 2 sent, 3 drafts, 4 outbox.
fn outgoing(kind: Option<i64>) -> bool {
    !matches!(kind, Some(1) | None)
}

struct SmsData {
    sms: Vec<Row>,
    mms: Vec<Row>,
    parts: HashMap<i64, Vec<Row>>,
    addrs: HashMap<i64, Vec<Row>>,
    names: HashMap<String, String>,
}

fn sms_data(path: &Path) -> Result<SmsData> {
    let files = read_files(path, &[SMS, MMS, MMS_PARTS, MMS_ADDR, RAW_CONTACTS, CONTACT_DATA], ROWS_LIMIT)?;
    let by = |rows: Vec<Row>, key: &str| {
        let mut map: HashMap<i64, Vec<Row>> = HashMap::new();
        for r in rows {
            if let Some(id) = int(&r, key) {
                map.entry(id).or_default().push(r);
            }
        }
        map
    };
    let contacts = build_contacts(&rows(&files, RAW_CONTACTS), &rows(&files, CONTACT_DATA));
    let mut parts = by(rows(&files, MMS_PARTS), "mid");
    for list in parts.values_mut() {
        list.sort_by_key(|p| int(p, "seq").unwrap_or(0));
    }
    Ok(SmsData {
        sms: rows(&files, SMS),
        mms: rows(&files, MMS),
        parts,
        addrs: by(rows(&files, MMS_ADDR), "_id_of"),
        names: names_by_number(&contacts),
    })
}

impl SmsData {
    fn mms_addresses(&self, id: i64, kind: i64) -> Vec<String> {
        self.addrs
            .get(&id)
            .into_iter()
            .flatten()
            .filter(|a| int(a, "type") == Some(kind))
            .filter_map(|a| text(a, "address"))
            .filter(|a| a != ADDR_TOKEN)
            .collect()
    }

    fn name(&self, address: &str) -> Option<String> {
        self.names.get(&number_key(address)).cloned()
    }
}

pub fn messages(path: &Path) -> Result<Vec<Conversation>> {
    let data = sms_data(path)?;
    let mut threads: BTreeMap<i64, Conversation> = BTreeMap::new();
    let mut add = |thread: i64, addresses: Vec<String>, msg: Message| {
        let c = threads.entry(thread).or_insert_with(|| Conversation {
            thread,
            addresses: vec![],
            names: vec![],
            last_date: 0,
            messages: vec![],
        });
        for a in addresses {
            // The same number may come with and without the country prefix.
            if !c.addresses.iter().any(|b| number_key(b) == number_key(&a)) {
                c.addresses.push(a);
            }
        }
        c.last_date = c.last_date.max(msg.date);
        c.messages.push(msg);
    };
    for r in &data.sms {
        let address = text(r, "address").unwrap_or_default();
        let thread = int(r, "thread_id").unwrap_or(0);
        let msg = Message {
            mms: false,
            date: int(r, "date").unwrap_or(0),
            outgoing: outgoing(int(r, "type")),
            from: None,
            body: text(r, "body").unwrap_or_default(),
            attachments: vec![],
        };
        add(thread, vec![address], msg);
    }
    for r in &data.mms {
        let Some(id) = int(r, "_id") else { continue };
        let out = outgoing(int(r, "msg_box"));
        let from = data.mms_addresses(id, ADDR_FROM).into_iter().next();
        // The TO list of a received MMS holds this phone's own number, which is not known here.
        let addresses = if out { data.mms_addresses(id, ADDR_TO) } else { from.iter().cloned().collect() };
        let mut body: Vec<String> = text(r, "sub").into_iter().collect();
        let mut attachments = vec![];
        for p in data.parts.get(&id).into_iter().flatten() {
            let mime = text(p, "ct").unwrap_or_default();
            match mime.as_str() {
                "text/plain" => body.extend(text(p, "text")),
                "application/smil" => {}
                _ => attachments.push(Attachment {
                    part: int(p, "_id").unwrap_or(0),
                    name: text(p, "name").or_else(|| text(p, "cl")).or_else(|| text(p, "fn")),
                    mime,
                }),
            }
        }
        let msg = Message {
            mms: true,
            // MMS dates are in seconds.
            date: int(r, "date").unwrap_or(0) * 1000,
            outgoing: out,
            from: if out { None } else { from },
            body: body.join("\n"),
            attachments,
        };
        add(int(r, "thread_id").unwrap_or(0), addresses, msg);
    }
    let mut out: Vec<Conversation> = threads.into_values().collect();
    for c in &mut out {
        c.messages.sort_by_key(|m| m.date);
        c.addresses.retain(|a| !a.is_empty());
        c.names = c.addresses.iter().map(|a| data.name(a)).collect();
    }
    out.sort_by_key(|c| std::cmp::Reverse(c.last_date));
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PartData {
    pub mime: String,
    pub base64: String,
}

/// The file of an MMS part: `_data` points into `app_parts/` on the phone.
fn part_file(path: &Path, part: i64) -> Result<(String, String)> {
    let files = read_files(path, &[MMS_PARTS], ROWS_LIMIT)?;
    let row = rows(&files, MMS_PARTS)
        .into_iter()
        .find(|r| int(r, "_id") == Some(part))
        .ok_or_else(|| Error::Io { reason: format!("MMS part {part}: not in the backup") })?;
    let data = text(&row, "_data").ok_or_else(|| Error::Io { reason: format!("MMS part {part}: no file") })?;
    let name = data.rsplit('/').next().unwrap_or(&data).to_string();
    Ok((name, text(&row, "ct").unwrap_or_default()))
}

/// Calls `f` with the entries of `app_parts.tar.zst` whose file name is in `names`.
fn read_parts(path: &Path, names: &HashSet<String>, limit: usize, mut f: impl FnMut(String, Vec<u8>)) -> Result<()> {
    with_file(path, APP_PARTS, |r| {
        let mut tar = tar::Archive::new(zstd::Decoder::new(r)?);
        for entry in tar.entries()? {
            let entry = entry?;
            let p = entry.path()?;
            let Some(name) = p.file_name().map(|n| n.to_string_lossy().into_owned()) else { continue };
            if names.contains(&name) {
                let mut buf = Vec::new();
                entry.take(limit as u64).read_to_end(&mut buf)?;
                f(name, buf);
            }
        }
        Ok(())
    })
}

pub fn mms_part(path: &Path, part: i64) -> Result<PartData> {
    let (name, mime) = part_file(path, part)?;
    let mut found = None;
    read_parts(path, &HashSet::from([name.clone()]), PART_LIMIT, |_, b| found = Some(b))?;
    let bytes = found.ok_or_else(|| Error::Io { reason: format!("{name}: not in the backup") })?;
    Ok(PartData { mime, base64: BASE64.encode(bytes) })
}

pub fn export_mms_part(path: &Path, part: i64, dest: &Path) -> Result<()> {
    let (name, _) = part_file(path, part)?;
    let mut found = None;
    read_parts(path, &HashSet::from([name.clone()]), usize::MAX, |_, b| found = Some(b))?;
    write_atomic(dest, &found.ok_or_else(|| Error::Io { reason: format!("{name}: not in the backup") })?)
}

/// Writes the messages as the XML of *SMS Backup & Restore*, which many apps import.
/// MMS attachments go inside as base64. Returns the number of messages.
pub fn export_sms_xml(path: &Path, dest: &Path) -> Result<u32> {
    let data = sms_data(path)?;
    let wanted: HashSet<String> = data
        .parts
        .values()
        .flatten()
        .filter_map(|p| text(p, "_data"))
        .map(|d| d.rsplit('/').next().unwrap_or(&d).to_string())
        .collect();
    let mut blobs: HashMap<String, Vec<u8>> = HashMap::new();
    if !wanted.is_empty() {
        // Old backups or phones without parts simply have no attachment data.
        let _ = read_parts(path, &wanted, usize::MAX, |n, b| {
            blobs.insert(n, b);
        });
    }

    enum Item<'a> {
        Sms(&'a Row),
        Mms(&'a Row),
    }
    let mut items: Vec<(i64, Item)> = data.sms.iter().map(|r| (int(r, "date").unwrap_or(0), Item::Sms(r))).collect();
    items.extend(data.mms.iter().map(|r| (int(r, "date").unwrap_or(0) * 1000, Item::Mms(r))));
    items.sort_by_key(|(d, _)| *d);

    let partial = crate::format::with_suffix(dest, "partial");
    let result = (|| -> Result<()> {
        let mut w = BufWriter::new(File::create(&partial)?);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        writeln!(w, "<?xml version='1.0' encoding='UTF-8' standalone='yes' ?>")?;
        writeln!(w, "<!--File created by adbkeep-->")?;
        writeln!(w, r#"<smses count="{}" backup_date="{now}" type="full">"#, items.len())?;
        let contact = |a: &str| data.name(a).unwrap_or_else(|| "(Unknown)".into());
        for (_, item) in &items {
            match item {
                Item::Sms(r) => {
                    let address = text(r, "address").unwrap_or_default();
                    write!(w, "  <sms")?;
                    for k in ["protocol", "address", "date", "type", "subject", "body", "toa", "sc_toa", "service_center", "read", "status", "locked", "date_sent", "sub_id"] {
                        attr(&mut w, k, &value_text(r.get(k)))?;
                    }
                    attr(&mut w, "contact_name", &contact(&address))?;
                    writeln!(w, " />")?;
                }
                Item::Mms(r) => {
                    let id = int(r, "_id").unwrap_or(0);
                    let mut addresses = data.mms_addresses(id, ADDR_TO);
                    addresses.extend(data.mms_addresses(id, ADDR_FROM));
                    addresses.dedup();
                    write!(w, "  <mms")?;
                    for (k, v) in r.iter().filter(|(k, _)| !k.starts_with('_') && *k != "thread_id") {
                        attr(&mut w, k, &value_text(Some(v)))?;
                    }
                    attr(&mut w, "address", &addresses.join("~"))?;
                    let names: Vec<String> = addresses.iter().map(|a| contact(a)).collect();
                    attr(&mut w, "contact_name", &names.join(", "))?;
                    writeln!(w, ">")?;
                    writeln!(w, "    <parts>")?;
                    for p in data.parts.get(&id).into_iter().flatten() {
                        write!(w, "      <part")?;
                        for k in ["seq", "ct", "name", "chset", "cd", "fn", "cid", "cl", "ctt_s", "ctt_t", "text"] {
                            attr(&mut w, k, &value_text(p.get(k)))?;
                        }
                        let file = text(p, "_data").map(|d| d.rsplit('/').next().unwrap_or(&d).to_string());
                        if let Some(bytes) = file.and_then(|f| blobs.get(&f)) {
                            attr(&mut w, "data", &BASE64.encode(bytes))?;
                        }
                        writeln!(w, " />")?;
                    }
                    writeln!(w, "    </parts>")?;
                    writeln!(w, "    <addrs>")?;
                    for a in data.addrs.get(&id).into_iter().flatten() {
                        write!(w, "      <addr")?;
                        for k in ["address", "type", "charset"] {
                            attr(&mut w, k, &value_text(a.get(k)))?;
                        }
                        writeln!(w, " />")?;
                    }
                    writeln!(w, "    </addrs>")?;
                    writeln!(w, "  </mms>")?;
                }
            }
        }
        writeln!(w, "</smses>")?;
        w.flush()?;
        Ok(())
    })();
    match result {
        Ok(()) => std::fs::rename(&partial, dest)?,
        Err(e) => {
            let _ = std::fs::remove_file(&partial);
            return Err(e);
        }
    }
    Ok(items.len() as u32)
}

/// Values the way SMS Backup & Restore writes them: missing ones as "null".
fn value_text(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => "null".into(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => (*b as u8).to_string(),
        Some(other) => other.to_string(),
    }
}

fn attr(w: &mut impl Write, name: &str, value: &str) -> Result<()> {
    let mut v = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => v.push_str("&amp;"),
            '<' => v.push_str("&lt;"),
            '>' => v.push_str("&gt;"),
            '"' => v.push_str("&quot;"),
            '\n' => v.push_str("&#10;"),
            '\r' => v.push_str("&#13;"),
            '\t' => v.push_str("&#9;"),
            // Not allowed in XML 1.0.
            c if (c as u32) < 0x20 => {}
            c => v.push(c),
        }
    }
    write!(w, r#" {name}="{v}""#)?;
    Ok(())
}

// ---------------------------------------------------------------- calls

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Call {
    pub number: String,
    pub name: Option<String>,
    pub date: i64,
    /// Seconds.
    pub duration: i64,
    /// `CallLog.Calls.TYPE`: 1 incoming, 2 outgoing, 3 missed, 4 voicemail, 5 rejected, 6 blocked.
    pub kind: i64,
}

pub fn calls(path: &Path) -> Result<Vec<Call>> {
    let files = read_files(path, &[CALLS, RAW_CONTACTS, CONTACT_DATA], ROWS_LIMIT)?;
    let names = names_by_number(&build_contacts(&rows(&files, RAW_CONTACTS), &rows(&files, CONTACT_DATA)));
    let mut out: Vec<Call> = rows(&files, CALLS)
        .iter()
        .map(|r| {
            let number = text(r, "number").unwrap_or_default();
            Call {
                name: text(r, "name").or_else(|| names.get(&number_key(&number)).cloned()),
                number,
                date: int(r, "date").unwrap_or(0),
                duration: int(r, "duration").unwrap_or(0),
                kind: int(r, "type").unwrap_or(0),
            }
        })
        .collect();
    out.sort_by_key(|c| std::cmp::Reverse(c.date));
    Ok(out)
}

// ---------------------------------------------------------------- Wi-Fi and settings

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WifiView {
    pub ssid: String,
    pub security: WifiSecurity,
    pub hidden: bool,
    pub has_password: bool,
    /// Only filled when asked to show the passwords.
    pub password: Option<String>,
}

pub fn wifi(path: &Path, reveal: bool) -> Result<Vec<WifiView>> {
    let files = read_files(path, &[WIFI], 16 * 1024 * 1024)?;
    let xml = files.get(WIFI).map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
    Ok(wifi::parse(&xml)
        .into_iter()
        .map(|n| WifiView {
            has_password: n.password.is_some(),
            password: if reveal { n.password } else { None },
            ssid: n.ssid,
            security: n.security,
            hidden: n.hidden,
        })
        .collect())
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SettingView {
    pub table: String,
    pub key: String,
    pub value: String,
    /// Marked for restoring when the backup was made.
    pub selected: bool,
}

pub fn settings(path: &Path) -> Result<Vec<SettingView>> {
    #[derive(Deserialize)]
    struct Saved {
        entries: Vec<crate::other::SettingEntry>,
        selected: Vec<String>,
    }
    let files = read_files(path, &[SETTINGS], 64 * 1024 * 1024)?;
    let Some(bytes) = files.get(SETTINGS) else { return Ok(vec![]) };
    let saved: Saved = serde_json::from_slice(bytes).map_err(|e| Error::UnexpectedOutput { what: format!("{SETTINGS}: {e}") })?;
    Ok(saved
        .entries
        .into_iter()
        .map(|e| SettingView { selected: saved.selected.contains(&format!("{}/{}", e.table, e.key)), table: e.table, key: e.key, value: e.value })
        .collect())
}

// ---------------------------------------------------------------- archives (shared files)

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveEntry {
    /// Path inside the archive, without "./" or a trailing "/".
    pub path: String,
    pub dir: bool,
    pub size: u64,
    /// Seconds since the epoch.
    pub mtime: i64,
}

fn clean(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    s.trim_start_matches("./").trim_end_matches('/').to_string()
}

/// Lists a `.tar.zst` of the backup (shared folders, app data…).
pub fn list_archive(path: &Path, rel: &str) -> Result<Vec<ArchiveEntry>> {
    check_archive(rel)?;
    with_file(path, rel, |r| {
        let mut tar = tar::Archive::new(zstd::Decoder::new(r)?);
        let mut out = Vec::new();
        for entry in tar.entries()? {
            let entry = entry?;
            let p = clean(&entry.path()?);
            if p.is_empty() || p == "." {
                continue;
            }
            let h = entry.header();
            out.push(ArchiveEntry {
                dir: h.entry_type().is_dir(),
                size: h.size().unwrap_or(0),
                mtime: h.mtime().unwrap_or(0) as i64,
                path: p,
            });
        }
        Ok(out)
    })
}

fn check_archive(rel: &str) -> Result<()> {
    if rel.ends_with(".tar.zst") { Ok(()) } else { Err(Error::Io { reason: format!("{rel}: not an archive") }) }
}

/// Copies the chosen files and folders of an archive into `dest`, each one by its name (a
/// chosen folder keeps its tree). Returns the number of files written.
pub fn extract(path: &Path, rel: &str, chosen: &[String], dest: &Path) -> Result<u32> {
    check_archive(rel)?;
    std::fs::create_dir_all(dest)?;
    with_file(path, rel, |r| {
        let mut tar = tar::Archive::new(zstd::Decoder::new(r)?);
        let mut count = 0;
        let mut written: HashSet<String> = HashSet::new();
        for entry in tar.entries()? {
            let mut entry = entry?;
            let p = clean(&entry.path()?);
            let Some(out) = chosen.iter().find_map(|c| target(&p, c)) else { continue };
            let mut out = portable_rel(&out, cfg!(windows));
            if entry.header().entry_type().is_file() {
                out = unique_rel(out, &mut written, CASE_INSENSITIVE);
            }
            let Some(out) = safe_join(dest, &out) else { continue };
            match entry.header().entry_type() {
                t if t.is_dir() => std::fs::create_dir_all(&out)?,
                t if t.is_file() => {
                    if let Some(parent) = out.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::io::copy(&mut entry, &mut BufWriter::new(File::create(&out)?))?;
                    count += 1;
                }
                // Links and devices are not copied out.
                _ => {}
            }
        }
        Ok(count)
    })
}

/// Where `entry` goes when `chosen` was picked: relative to the folder holding `chosen`.
fn target(entry: &str, chosen: &str) -> Option<String> {
    if entry != chosen && !entry.starts_with(&format!("{chosen}/")) {
        return None;
    }
    let base = chosen.rfind('/').map(|i| i + 1).unwrap_or(0);
    Some(entry[base..].to_string())
}

/// The destination can hold names that differ only in case (Windows, macOS).
const CASE_INSENSITIVE: bool = cfg!(any(windows, target_os = "macos"));

/// One path component that Windows accepts: forbidden characters become `_`, trailing dots and
/// spaces are dropped and reserved device names get a `_` prefix.
fn portable_component(c: &str) -> String {
    let mut s: String = c.chars().map(|ch| if ch.is_control() || "<>:\"\\|?*".contains(ch) { '_' } else { ch }).collect();
    s.truncate(s.trim_end_matches(['.', ' ']).len());
    if s.is_empty() {
        return "_".into();
    }
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.as_bytes()[3].is_ascii_digit());
    if reserved { format!("_{s}") } else { s }
}

/// `rel` ("/"-separated) with every component made portable when `windows`.
fn portable_rel(rel: &str, windows: bool) -> String {
    if !windows {
        return rel.to_string();
    }
    rel.split('/').map(portable_component).collect::<Vec<_>>().join("/")
}

/// `rel`, or `name (2).ext`, `name (3).ext`… when that file was already written in this
/// extraction (comparing case-insensitively if `fold`), so two files never overwrite each other.
fn unique_rel(rel: String, written: &mut HashSet<String>, fold: bool) -> String {
    let key = |s: &str| if fold { s.to_lowercase() } else { s.to_string() };
    if written.insert(key(&rel)) {
        return rel;
    }
    let (dir, name) = rel.rsplit_once('/').map(|(d, n)| (format!("{d}/"), n)).unwrap_or((String::new(), rel.as_str()));
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    for n in 2.. {
        let candidate = format!("{dir}{stem} ({n}){ext}");
        if written.insert(key(&candidate)) {
            return candidate;
        }
    }
    unreachable!()
}

fn safe_join(dest: &Path, rel: &str) -> Option<PathBuf> {
    let rel = Path::new(rel);
    if rel.components().all(|c| matches!(c, Component::Normal(_))) { Some(dest.join(rel)) } else { None }
}

fn write_atomic(dest: &Path, bytes: &[u8]) -> Result<()> {
    let partial = crate::format::with_suffix(dest, "partial");
    std::fs::write(&partial, bytes)?;
    std::fs::rename(&partial, dest)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(json: &str) -> Row {
        serde_json::from_str(json).unwrap()
    }

    fn files(entries: &[(&str, &str)]) -> HashMap<String, Vec<u8>> {
        entries.iter().map(|(k, v)| (k.to_string(), v.as_bytes().to_vec())).collect()
    }

    #[test]
    fn contacts_merge_raw_contacts_and_fields() {
        let raw = [
            row(r#"{"_id":1,"contact_id":10,"account_type":null,"display_name":"Ana","starred":0}"#),
            row(r#"{"_id":2,"contact_id":10,"account_type":"com.google","display_name":"Ana","starred":1}"#),
            row(r#"{"_id":3,"contact_id":11,"account_type":null,"display_name":"bob"}"#),
        ];
        let data = [
            row(r#"{"raw_contact_id":1,"mimetype":"vnd.android.cursor.item/name","data1":"Ana López"}"#),
            row(r#"{"raw_contact_id":1,"mimetype":"vnd.android.cursor.item/phone_v2","data1":"+34 600 111 222","data2":2}"#),
            row(r#"{"raw_contact_id":2,"mimetype":"vnd.android.cursor.item/phone_v2","data1":"+34 600 111 222","data2":2}"#),
            row(r#"{"raw_contact_id":2,"mimetype":"vnd.android.cursor.item/email_v2","data1":"a@x.es","data2":0,"data3":"Club"}"#),
            row(r#"{"raw_contact_id":2,"mimetype":"vnd.android.cursor.item/organization","data1":"ACME","data4":"Jefa"}"#),
            row(r#"{"raw_contact_id":3,"mimetype":"vnd.android.cursor.item/photo","data15":{"base64":"AAEC"}}"#),
        ];
        let c = build_contacts(&raw, &data);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].name, "Ana López");
        assert_eq!(c[0].accounts, ["", "com.google"]);
        assert!(c[0].starred);
        assert_eq!(c[0].fields.len(), 3, "{:?}", c[0].fields);
        assert_eq!(c[0].fields[1].label.as_deref(), Some("Club"));
        assert_eq!(c[0].fields[2].value, "ACME · Jefa");
        assert_eq!(c[1].photo.as_deref(), Some("AAEC"));
        let names = names_by_number(&c);
        assert_eq!(names.get(&number_key("600111222")).map(String::as_str), Some("Ana López"));
        let vcf = to_vcard(&c);
        assert_eq!(vcf.matches("BEGIN:VCARD").count(), 2);
        assert!(vcf.contains("TEL;TYPE=CELL:+34 600 111 222\r\n"));
    }

    #[test]
    fn xml_attributes_are_escaped() {
        let mut out = Vec::new();
        attr(&mut out, "body", "a<b & \"c\"\nd\u{1}").unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), r#" body="a&lt;b &amp; &quot;c&quot;&#10;d""#);
        assert_eq!(value_text(None), "null");
        assert_eq!(value_text(Some(&Value::from(5))), "5");
    }

    #[test]
    fn extract_targets_stay_inside() {
        assert_eq!(target("DCIM/Camera/a.jpg", "DCIM/Camera").as_deref(), Some("Camera/a.jpg"));
        assert_eq!(target("DCIM/Camera/a.jpg", "DCIM/Camera/a.jpg").as_deref(), Some("a.jpg"));
        assert_eq!(target("DCIM/CameraX/a.jpg", "DCIM/Camera"), None);
        assert_eq!(target("notes.txt", "notes.txt").as_deref(), Some("notes.txt"));
        assert!(safe_join(Path::new("/tmp/x"), "../etc").is_none());
    }

    #[test]
    fn rows_skip_bad_lines() {
        let f = files(&[("a", "{\"x\":1}\nnot json\n{\"x\":\"2\"}\n")]);
        let r = rows(&f, "a");
        assert_eq!(r.len(), 2);
        assert_eq!(int(&r[1], "x"), Some(2));
    }

    /// A backup folder with SMS, an MMS with a picture, contacts, calls and shared files,
    /// read through the public functions (the packed variant is covered by `read_files`).
    #[test]
    fn whole_backup() {
        let dir = std::env::temp_dir().join(format!("adbkeep-viewer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let w = |rel: &str, body: &[u8]| {
            let p = dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        };
        w(SMS, br#"{"_id":1,"thread_id":5,"address":"600111222","date":1000,"type":1,"body":"hola","read":1}
{"_id":2,"thread_id":5,"address":"600111222","date":3000,"type":2,"body":"adios"}
"#);
        w(MMS, br#"{"_id":9,"thread_id":5,"date":2,"msg_box":1,"sub":null,"m_type":132}
"#);
        w(MMS_PARTS, br#"{"_id":40,"mid":9,"seq":0,"ct":"text/plain","text":"mira"}
{"_id":41,"mid":9,"seq":1,"ct":"image/jpeg","name":"foto.jpg","_data":"/data/user_de/0/com.android.providers.telephony/app_parts/PART_41"}
"#);
        w(MMS_ADDR, br#"{"_id_of":"9","address":"+34600111222","type":137}
{"_id_of":"9","address":"insert-address-token","type":151}
"#);
        w(RAW_CONTACTS, br#"{"_id":1,"contact_id":1,"display_name":"Ana"}
"#);
        w(CONTACT_DATA, br#"{"raw_contact_id":1,"mimetype":"vnd.android.cursor.item/phone_v2","data1":"600 111 222","data2":2}
"#);
        w(CALLS, br#"{"number":"600111222","date":5,"duration":61,"type":3}
"#);
        let tar_zst = |entries: &[(&str, &[u8])]| {
            let mut t = tar::Builder::new(zstd::Encoder::new(Vec::new(), 3).unwrap());
            for (name, data) in entries {
                let mut h = tar::Header::new_gnu();
                h.set_size(data.len() as u64);
                h.set_mode(0o644);
                h.set_cksum();
                t.append_data(&mut h, name, *data).unwrap();
            }
            t.into_inner().unwrap().finish().unwrap()
        };
        w(APP_PARTS, &tar_zst(&[("app_parts/PART_41", b"JPEGDATA")]));
        w("other/files/DCIM.tar.zst", &tar_zst(&[("DCIM/Camera/a.jpg", b"A"), ("DCIM/Camera/b.jpg", b"BB"), ("DCIM/x.txt", b"x")]));

        let conv = messages(&dir).unwrap();
        assert_eq!(conv.len(), 1);
        let c = &conv[0];
        assert_eq!((c.addresses.as_slice(), c.names.as_slice()), (&["600111222".to_string()][..], &[Some("Ana".to_string())][..]));
        assert_eq!(c.messages.iter().map(|m| m.body.as_str()).collect::<Vec<_>>(), ["hola", "mira", "adios"]);
        assert_eq!(c.messages[1].attachments[0].part, 41);
        assert_eq!(mms_part(&dir, 41).unwrap().base64, BASE64.encode(b"JPEGDATA"));

        let xml_path = dir.join("sms.xml");
        assert_eq!(export_sms_xml(&dir, &xml_path).unwrap(), 3);
        let xml = std::fs::read_to_string(&xml_path).unwrap();
        assert!(xml.contains(r#"<smses count="3""#));
        assert!(xml.contains(r#"body="hola""#) && xml.contains(r#"contact_name="Ana""#));
        assert!(xml.contains(&format!(r#"data="{}""#, BASE64.encode(b"JPEGDATA"))));
        assert!(xml.contains(r#"m_type="132""#) && !xml.contains("thread_id="));

        let calls = calls(&dir).unwrap();
        assert_eq!((calls[0].name.as_deref(), calls[0].kind, calls[0].duration), (Some("Ana"), 3, 61));

        let vcf = dir.join("c.vcf");
        assert_eq!(export_vcf(&dir, &vcf).unwrap(), 1);

        let list = list_archive(&dir, "other/files/DCIM.tar.zst").unwrap();
        assert_eq!(list.len(), 3);
        let out = dir.join("out");
        assert_eq!(extract(&dir, "other/files/DCIM.tar.zst", &["DCIM/Camera".into()], &out).unwrap(), 2);
        assert_eq!(std::fs::read(out.join("Camera/b.jpg")).unwrap(), b"BB");
        assert!(list_archive(&dir, SMS).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn extraction_names_are_portable_and_unique() {
        assert_eq!(portable_rel("DCIM/a:b?.jpg", true), "DCIM/a_b_.jpg");
        assert_eq!(portable_rel("x./CON.txt/end. ", true), "x/_CON.txt/end");
        assert_eq!(portable_rel("a:b", false), "a:b");
        let mut seen = HashSet::new();
        assert_eq!(unique_rel("d/Foto.JPG".into(), &mut seen, true), "d/Foto.JPG");
        assert_eq!(unique_rel("d/foto.jpg".into(), &mut seen, true), "d/foto (2).jpg");
        assert_eq!(unique_rel("d/foto.jpg".into(), &mut seen, true), "d/foto (3).jpg");
        let mut seen = HashSet::new();
        assert_eq!(unique_rel("a".into(), &mut seen, false), "a");
        assert_eq!(unique_rel("A".into(), &mut seen, false), "A");
    }
}
