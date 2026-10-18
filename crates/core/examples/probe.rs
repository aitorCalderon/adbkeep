//! Probes the first connected phone (read-only) and prints what the UI would show.
//! `cargo run -p adbkeep-core --example probe`

use std::sync::Arc;

use adbkeep_core::adb::{self, Adb};
use adbkeep_core::device::{Device, probe};
use adbkeep_core::log::CommandLog;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let loc = adb::locate(None, None)?;
    let adb = Adb::new(loc.path, Arc::new(CommandLog::new()));
    let serial = adb.devices().await?.into_iter().next().ok_or("no device")?.serial;
    let device = Device::new(adb, serial);
    let info = probe::probe(&device).await?;
    println!("{info:#?}");
    println!("{:#?}", probe::probe_root(&device, &info).await?);
    Ok(())
}
