//! What is installed on a restore target, to decide what to install and what to skip.

use std::collections::HashMap;

use super::access::ROLES;
use super::packages::{parse_packages, parse_pm_list_uids};
use super::split_sections;
use crate::Result;
use crate::device::Device;

#[derive(Debug, Clone, PartialEq)]
pub struct InstalledPackage {
    pub uid: Option<u32>,
    pub version_code: u64,
    pub signatures: Vec<String>,
    pub system: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Installed {
    pub packages: HashMap<String, InstalledPackage>,
    /// Role (`android.app.role.BROWSER`) → current holders.
    pub roles: HashMap<String, Vec<String>>,
}

/// All packages of user 0 and the default-app role holders.
pub async fn installed(device: &Device) -> Result<Installed> {
    let uids: HashMap<String, u32> =
        parse_pm_list_uids(&device.shell("pm list packages -U").await?.ok()?.stdout_str()).into_iter().collect();
    let dump = device.shell("dumpsys package packages").await?.ok()?.stdout_str();
    let packages = parse_packages(&dump, 0)
        .into_iter()
        .filter(|p| uids.contains_key(&p.package))
        .map(|p| {
            let system = p.is_system();
            (
                p.package.clone(),
                InstalledPackage { uid: uids.get(&p.package).copied(), version_code: p.version_code, signatures: p.signatures, system },
            )
        })
        .collect();
    let mut script = String::new();
    for r in ROLES {
        script.push_str(&format!("echo @@android.app.role.{r}; cmd role get-role-holders android.app.role.{r}; "));
    }
    let roles = split_sections(&device.shell(&script).await?.stdout_str())
        .into_iter()
        .map(|(role, body)| (role, body.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect()))
        .collect();
    Ok(Installed { packages, roles })
}
