//! Backs up the given packages (APK + all data + permissions) of the first phone into a folder.
//! Closes those apps (`am force-stop`) while copying their data.
//! `cargo run -p adbkeep-core --example backup -- <dest_dir> [--pack [--password <pw>]] [--helper <dex>] [--other a,b,files:DCIM] <pkg>...`
//! Resto items: sms, contacts (phone-only), calls, blocked, calendar, wifi, settings (whitelist),
//! notifications, sounds, wallpaper, dictionary, certificates, files:<folder>.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use adbkeep_core::adb::{self, Adb};
use adbkeep_core::backup::{self, AppSelection, BackupRequest, JobEvent};
use adbkeep_core::device::{Device, probe};
use adbkeep_core::format::DataPart;
use adbkeep_core::log::CommandLog;
use adbkeep_core::analysis;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let pack = args.iter().any(|a| a == "--pack");
    args.retain(|a| a != "--pack");
    let mut take = |flag: &str| {
        let i = args.iter().position(|a| a == flag)?;
        let v = args.get(i + 1).cloned();
        args.drain(i..(i + 2).min(args.len()));
        v
    };
    let helper = take("--helper").map(std::path::PathBuf::from);
    let other_arg = take("--other").unwrap_or_default();
    let password = take("--password");
    let resume = take("--resume");
    let dest = args.remove(0);
    let loc = adb::locate(None, None)?;
    let adb = Adb::new(loc.path, Arc::new(CommandLog::new()));
    let serial = adb.devices().await?.into_iter().next().ok_or("no device")?.serial;
    let device = Device::new(adb, serial);
    let info = probe::probe(&device).await?;
    let root = probe::probe_root(&device, &info).await?;
    let device = device.with_root(root.method);
    let tmp = std::env::temp_dir().join("adbkeep-ex");
    let a = analysis::analyze(&device, info.sdk, &tmp, helper.as_deref(), |_| {}).await?;
    let mut other = adbkeep_core::other::backup::OtherSelection { exclude: vec![".thumbnails".into()], ..Default::default() };
    for item in other_arg.split(',').filter(|s| !s.is_empty()) {
        match item {
            "sms" => other.sms = true,
            "contacts" => other.contacts = vec!["".into(), "com.android.contacts".into()],
            "calls" => other.calls = true,
            "blocked" => other.blocked = true,
            "calendar" => other.calendar = true,
            "wifi" => other.wifi = true,
            "settings" => {
                other.settings = Some(adbkeep_core::other::SETTINGS_WHITELIST.iter().map(|(t, k)| format!("{t}/{k}")).collect())
            }
            "notifications" => other.notifications = true,
            "sounds" => other.sounds = true,
            "wallpaper" => other.wallpaper = true,
            "dictionary" => other.dictionary = true,
            "certificates" => other.certificates = true,
            f if f.starts_with("files:") => other.folders.push(f[6..].to_string()),
            other => return Err(format!("unknown item {other}").into()),
        }
    }
    let req = BackupRequest {
        dest_dir: dest,
        name: format!("{}_example", info.model.replace(' ', "_")),
        apps: args
            .iter()
            .map(|p| AppSelection {
                package: p.clone(),
                apk: true,
                data: DataPart::ALL.to_vec(),
                permissions: true,
                excluded_permissions: vec![],
            })
            .collect(),
        include_no_backup: false,
        compression_level: 3,
        pack,
        other,
        password,
        resume,
    };
    for cmd in backup::simulate(&device, &backup::plan(&a.apps, &req)) {
        println!("plan: {cmd}");
    }
    let cancel = AtomicBool::new(false);
    let extras = backup::Extras { helper_dex: helper.as_deref(), tmp: &tmp, gate: None };
    let result = backup::run(&device, &info, &a, &req, &extras, &cancel, |e| match e {
        JobEvent::Progress { .. } => {}
        other => println!("{other:?}"),
    })
    .await?;
    println!("{result:#?}");
    Ok(())
}
