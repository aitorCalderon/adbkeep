//! Read-only: compatibility of a backup's apps with the first phone, and the commands a full
//! restore would run. Nothing is written to the phone.
//! `cargo run -p adbkeep-core --example restore_preview -- <backup>`

use std::collections::HashMap;
use std::sync::Arc;

use adbkeep_core::adb::{self, Adb};
use adbkeep_core::compat::CompatOptions;
use adbkeep_core::device::{Device, probe};
use adbkeep_core::log::CommandLog;
use adbkeep_core::restore::{self, RestoreRequest, RestoreSelection};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::path::PathBuf::from(std::env::args().nth(1).ok_or("usage: restore_preview <backup>")?);
    let adb = Adb::new(adb::locate(None, None)?.path, Arc::new(CommandLog::new()));
    let serial = adb.devices().await?.into_iter().next().ok_or("no device")?.serial;
    let device = Device::new(adb, serial);
    let info = probe::probe(&device).await?;
    let p = restore::preview(&device, &info, &path, CompatOptions::default(), true).await?;
    for o in &p.other {
        println!("other {:?}: {:?} {:?} count={:?}", o.item, o.status, o.reasons, o.count);
    }
    let all_other = adbkeep_core::other::restore::OtherRestoreSelection {
        sms: true,
        contacts: true,
        calls: true,
        blocked: true,
        calendar: true,
        dictionary: true,
        wifi: true,
        settings: vec!["system/font_scale".into()],
        sounds: true,
        wallpaper: true,
        certificates: true,
        folders: vec!["Pictures".into()],
        ..Default::default()
    };
    let wifi = restore::wifi_networks(&path, &p.manifest);
    for item in all_other.items() {
        for line in adbkeep_core::other::restore::describe(item, &all_other, &wifi) {
            println!("  [{item:?}] {line}");
        }
    }
    for a in &p.apps {
        println!("{:<28} {:?} {:?} data={} installed={:?} reasons={:?}", a.meta.package, a.compat.status, a.compat.action, a.compat.data_allowed, a.compat.installed_version, a.compat.reasons);
    }
    let req = RestoreRequest {
        backup_path: path.display().to_string(),
        apps: p.manifest.apps.iter().map(|a| RestoreSelection {
            package: a.package.clone(),
            app: true,
            data: a.data.iter().map(|d| d.part).collect(),
            permissions: true,
            excluded_permissions: vec![],
        }).collect(),
        install_as_play: false,
        bypass_low_target_sdk: false,
        other: Default::default(),
    };
    let metas: HashMap<_, _> = p.apps.iter().map(|a| (a.meta.package.clone(), a.meta.clone())).collect();
    let compat: Vec<_> = p.apps.iter().map(|a| a.compat.clone()).collect();
    for s in restore::plan(&p.manifest, &metas, &compat, &req, &restore::file_sizes(&p.manifest)) {
        for c in &s.commands {
            println!("  [{}] {c}", s.package);
        }
    }
    Ok(())
}
