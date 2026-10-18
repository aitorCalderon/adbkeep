//! Tries the on-device helper on the first phone: copies the dex to /data/local/tmp/adbkeep,
//! only reads, and removes it. Prints counts and shapes, never personal content.
//! `cargo run -p adbkeep-core --example helper_probe -- <dex> [extra content URIs to count]`

use std::sync::Arc;

use adbkeep_core::adb::{self, Adb};
use adbkeep_core::device::{Device, probe};
use adbkeep_core::helper::Helper;
use adbkeep_core::log::CommandLog;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dex = std::path::PathBuf::from(std::env::args().nth(1).ok_or("usage: helper_probe <dex>")?);
    let adb = Adb::new(adb::locate(None, None)?.path, Arc::new(CommandLog::new()));
    let serial = adb.devices().await?.into_iter().next().ok_or("no device")?.serial;
    let device = Device::new(adb, serial);
    let info = probe::probe(&device).await?;
    let root = probe::probe_root(&device, &info).await?.method;
    let device = device.with_root(root);
    let tmp = std::env::temp_dir().join("adbkeep-helper-probe");
    let helper = Helper::deploy(&device, &dex, &tmp).await?;
    println!("deployed, protocol ok");

    let result = async {
        let pkgs: Vec<String> = ["com.termux", "com.android.chrome", "com.topjohnwu.magisk", "de.blinkt.openvpn", "no.such.package"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut labels: Vec<_> = helper.labels(&pkgs).await?.into_values().collect();
        labels.sort_by(|a, b| a.package.cmp(&b.package));
        for l in &labels {
            println!("label {:<24} {:?} icon={}B error={:?}", l.package, l.label, l.icon.as_ref().map(|i| i.len()).unwrap_or(0), l.error);
        }
        if let Some(icon) = labels.iter().find_map(|l| l.icon.clone()) {
            use std::io::Write;
            let png = tmp.join("icon.b64");
            std::fs::File::create(&png)?.write_all(icon.as_bytes())?;
            println!("sample icon saved to {}", png.display());
        }
        for uri in std::env::args().skip(2) {
            let rows = helper.query(&uri, None, None, None).await?;
            let cols = rows.first().map(|r| r.len()).unwrap_or(0);
            let types: std::collections::BTreeSet<&str> = rows
                .iter()
                .flat_map(|r| r.values())
                .map(|v| match v {
                    serde_json::Value::Null => "null",
                    serde_json::Value::Number(_) => "number",
                    serde_json::Value::String(_) => "string",
                    serde_json::Value::Object(_) => "blob",
                    _ => "other",
                })
                .collect();
            println!("query {uri} rows={} columns={cols} value types={types:?}", rows.len());
        }
        for (name, uri) in [
            ("contacts", "content://com.android.contacts/raw_contacts"),
            ("sms", "content://sms"),
            ("calls", "content://call_log/calls"),
            ("blocked", "content://com.android.blockednumber/blocked"),
            ("calendar", "content://com.android.calendar/events"),
            ("dictionary", "content://user_dictionary/words"),
        ] {
            match helper.query(uri, None, None, None).await {
                Ok(rows) => {
                    let cols = rows.first().map(|r| r.len()).unwrap_or(0);
                    let multiline = rows.iter().filter(|r| r.values().any(|v| v.as_str().is_some_and(|s| s.contains('\n')))).count();
                    println!("query {name:<10} rows={} columns={cols} rows_with_multiline_text={multiline}", rows.len());
                }
                Err(e) => println!("query {name:<10} error {e}"),
            }
        }
        // A vCard of up to 3 contacts, by lookup key.
        let contacts = helper.query("content://com.android.contacts/contacts", Some(&["lookup"]), None, None).await?;
        let keys: Vec<&str> = contacts.iter().filter_map(|r| r.get("lookup")?.as_str()).take(3).collect();
        if !keys.is_empty() {
            let uri = format!("content://com.android.contacts/contacts/as_multi_vcard/{}", keys.join(":"));
            let dest = tmp.join("contacts.vcf");
            let r = helper.asset_to_file(&uri, &dest).await?;
            let text = std::fs::read_to_string(&dest)?;
            println!("vcard {} contacts -> {} bytes, BEGIN:VCARD x{}", keys.len(), r.bytes_in, text.matches("BEGIN:VCARD").count());
            std::fs::remove_file(&dest)?;
        } else {
            println!("vcard: no contacts");
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    }
    .await;
    helper.remove().await?;
    let left = device.shell("ls -d /data/local/tmp/adbkeep 2>&1").await?.stdout_str();
    println!("removed; /data/local/tmp/adbkeep -> {}", left.trim());
    result
}
