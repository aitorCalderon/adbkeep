//! Really restores a backup (apps + contacts + sms + calls) onto the first phone.
//! `cargo run -p adbkeep-core --example restore_run -- <backup> <helper.dex>`

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use adbkeep_core::adb::{self, Adb};
use adbkeep_core::backup::{Extras, JobEvent};
use adbkeep_core::compat::CompatOptions;
use adbkeep_core::device::{Device, probe};
use adbkeep_core::log::CommandLog;
use adbkeep_core::restore::{self, RestoreRequest, RestoreSelection};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut a = std::env::args().skip(1);
    let path = std::path::PathBuf::from(a.next().ok_or("usage: restore_run <backup> <helper.dex>")?);
    let helper = std::path::PathBuf::from(a.next().ok_or("helper")?);
    let adb = Adb::new(adb::locate(None, None)?.path, Arc::new(CommandLog::new()));
    let serial = adb.devices().await?.into_iter().next().ok_or("no device")?.serial;
    let device = Device::new(adb, serial);
    let info = probe::probe(&device).await?;
    let root = probe::probe_root(&device, &info).await?;
    let device = device.with_root(root.method);
    let p = restore::preview(&device, &info, &path, CompatOptions::default(), true).await?;
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
        other: adbkeep_core::other::restore::OtherRestoreSelection {
            sms: true, contacts: true, calls: true, blocked: true, calendar: true, dictionary: true, wifi: true,
            settings: adbkeep_core::other::SETTINGS_WHITELIST.iter().map(|(t, k)| format!("{t}/{k}")).collect(),
            sounds: true, wallpaper: true, certificates: true,
            folders: ["DCIM", "Documents", "Music", "Pictures", "Download", ""].iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        },
    };
    let metas: HashMap<_, _> = p.apps.iter().map(|a| (a.meta.package.clone(), a.meta.clone())).collect();
    let compat: Vec<_> = p.apps.iter().map(|a| a.compat.clone()).collect();
    let steps = restore::plan(&p.manifest, &metas, &compat, &req, &restore::file_sizes(&p.manifest));
    let tmp = std::env::temp_dir().join("adbkeep-ex");
    let extras = Extras { helper_dex: Some(helper.as_path()), tmp: &tmp, gate: None };
    let cancel = AtomicBool::new(false);
    let r = restore::run(&device, info.sdk, &path, &p.manifest, &steps, &req, &extras, &cancel, |e| match e {
        JobEvent::Progress { .. } => {}
        o => println!("{o:?}"),
    })
    .await?;
    println!("{r:#?}");
    Ok(())
}
