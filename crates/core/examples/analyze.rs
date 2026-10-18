//! Analyses the first connected phone (read-only) and prints a summary per app.
//! `cargo run -p adbkeep-core --example analyze [-- --helper <dex>]`
//! With `--helper`, the helper dex is copied to /data/local/tmp/adbkeep and removed afterwards.

use std::sync::Arc;

use adbkeep_core::adb::{self, Adb};
use adbkeep_core::analysis;
use adbkeep_core::device::{Device, probe};
use adbkeep_core::log::CommandLog;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let loc = adb::locate(None, None)?;
    let adb = Adb::new(loc.path, Arc::new(CommandLog::new()));
    let serial = adb.devices().await?.into_iter().next().ok_or("no device")?.serial;
    let device = Device::new(adb, serial);
    let info = probe::probe(&device).await?;
    let root = probe::probe_root(&device, &info).await?;
    let device = device.with_root(root.method);
    let start = std::time::Instant::now();
    let tmp = std::env::temp_dir().join("adbkeep-analyze-example");
    let args: Vec<String> = std::env::args().collect();
    let helper = args.iter().position(|a| a == "--helper").and_then(|i| args.get(i + 1)).map(std::path::PathBuf::from);
    let a = analysis::analyze(&device, info.sdk, &tmp, helper.as_deref(), |a| eprintln!("stage {:?} at {:?}", a.stage, start.elapsed())).await?;
    for app in &a.apps {
        println!(
            "{:<32} {:?} icon={} {:?} {:?} v{:<10} apk {:>4} files {:>10} B  data {:?}  perms {} ops {:?} special {:?} roles {:?} keystore {}",
            app.package,
            app.label,
            app.icon.as_ref().map(|i| i.len()).unwrap_or(0),
            app.kind,
            app.exclusion,
            app.version_code,
            app.apks.len(),
            app.apk_size(),
            app.data,
            app.permissions.runtime.len(),
            app.permissions.app_ops,
            app.permissions.special,
            app.permissions.roles,
            app.keystore
        );
    }
    println!("accounts: {:?}\nerrors: {:?}", a.accounts, a.errors);
    Ok(())
}
