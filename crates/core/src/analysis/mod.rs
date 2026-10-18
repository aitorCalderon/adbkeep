//! Analysing the phone before a backup (ui-spec §3: "al conectar el móvil se analiza").

pub mod access;
pub mod installed;
pub mod keystore;
pub mod packages;

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use serde::{Deserialize, Serialize};

pub use access::{Account, AppOp, SpecialAccess};

use crate::device::Device;
use crate::format::DataPart;
use crate::root::shell_quote;
use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum AppKind {
    /// `pm list packages -3`.
    User,
    /// System app updated with its APK in `/data/app` (backup-android.md, system-apps section).
    UpdatedSystem,
}

/// Why an updated system app is left out by default (backup-android.md, system-apps section).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Exclusion {
    /// GMS, GSF, Play Store, WebView…: tied to the device or broken if copied.
    FixedList,
    /// Shares a system UID (`android.uid.*`).
    #[serde(rename_all = "camelCase")]
    SystemSharedUser { shared_user: String },
}

/// Packages never backed up by default; `*` matches a prefix (Trichrome has a version suffix).
pub const EXCLUDED_SYSTEM_APPS: &[&str] = &[
    "com.google.android.gms",
    "com.google.android.gsf",
    "com.android.vending",
    "com.google.android.webview",
    "com.google.android.trichromelibrary*",
    "com.google.android.as",
    "com.google.android.as.oss",
    "com.google.android.ims",
    "com.google.android.apps.messaging",
    // Google system services seen on a Pixel 10 Pro (Android 16); decided on 2026-10-14.
    "com.google.android.euicc",
    "com.google.android.verifier",
    "com.google.android.partnersetup",
    "com.google.android.aicore",
    "com.google.android.settings.intelligence",
    "com.google.android.odad",
];

pub fn exclusion(package: &str, shared_user: Option<&str>) -> Option<Exclusion> {
    let listed = EXCLUDED_SYSTEM_APPS.iter().any(|e| match e.strip_suffix('*') {
        Some(prefix) => package.starts_with(prefix),
        None => package == *e,
    });
    if listed {
        return Some(Exclusion::FixedList);
    }
    shared_user
        .filter(|s| s.starts_with("android.uid."))
        .map(|s| Exclusion::SystemSharedUser { shared_user: s.to_string() })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ApkFile {
    /// File name inside `codePath` (`base.apk`, `split_config.arm64_v8a.apk`…).
    pub name: String,
    pub size: u64,
}

/// Sizes in bytes of each data location; `None` when it does not exist (backup-android.md point 2).
/// They leave out what a backup skips ([`DataPart::excludes`]: caches, `lib`, `no_backup`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DataSizes {
    /// `/data/data/<pkg>` (credential-encrypted).
    pub ce: Option<u64>,
    /// `/data/user_de/0/<pkg>` (device-encrypted).
    pub de: Option<u64>,
    /// `/sdcard/Android/data/<pkg>`.
    pub ext_data: Option<u64>,
    /// `/sdcard/Android/obb/<pkg>`.
    pub obb: Option<u64>,
    /// `/sdcard/Android/media/<pkg>`.
    pub media: Option<u64>,
    /// `no_backup` inside `ce`, left out of `ce` and only saved on request.
    #[serde(default)]
    pub ce_no_backup: u64,
    /// `no_backup` inside `de`, left out of `de` and only saved on request.
    #[serde(default)]
    pub de_no_backup: u64,
}

impl DataSizes {
    pub fn total(&self) -> u64 {
        [self.ce, self.de, self.ext_data, self.obb, self.media].iter().flatten().sum()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Permissions {
    /// Granted runtime permissions.
    pub runtime: Vec<String>,
    pub app_ops: Vec<AppOp>,
    pub special: Vec<SpecialAccess>,
    /// Default-app roles held, e.g. `android.app.role.BROWSER`.
    pub roles: Vec<String>,
}

impl Permissions {
    pub fn is_empty(&self) -> bool {
        self.runtime.is_empty() && self.app_ops.is_empty() && self.special.is_empty() && self.roles.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub package: String,
    /// Name shown on the phone (from the on-device helper); `None` falls back to the package.
    #[serde(default)]
    pub label: Option<String>,
    /// 96×96 PNG, base64.
    #[serde(default)]
    pub icon: Option<String>,
    pub kind: AppKind,
    pub uid: u32,
    pub version_code: u64,
    pub version_name: Option<String>,
    pub min_sdk: u32,
    pub target_sdk: u32,
    pub code_path: String,
    pub apks: Vec<ApkFile>,
    pub splits: Vec<String>,
    pub primary_abi: Option<String>,
    pub installer: Option<String>,
    pub signatures: Vec<String>,
    pub shared_user: Option<String>,
    pub flags: Vec<String>,
    pub data: DataSizes,
    pub permissions: Permissions,
    /// Has keys in Android Keystore: its data may not work on another phone (point 18).
    pub keystore: bool,
    /// Updated system apps left out by default, and why.
    #[serde(default)]
    pub exclusion: Option<Exclusion>,
}

impl AppInfo {
    pub fn apk_size(&self) -> u64 {
        self.apks.iter().map(|a| a.size).sum()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum AnalysisStage {
    Packages,
    Labels,
    Sizes,
    Permissions,
    Keystore,
    /// Counts and sizes for the Resto tab.
    Other,
    Done,
}

/// Grows as each stage finishes, so the UI can fill the tabs progressively.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Analysis {
    /// The stage being run next (`Done` when finished).
    pub stage: AnalysisStage,
    pub apps: Vec<AppInfo>,
    pub accounts: Vec<Account>,
    pub other: Option<crate::other::OtherAnalysis>,
    /// Errors of stages that failed; the analysis goes on without them.
    pub errors: Vec<StageError>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StageError {
    pub stage: AnalysisStage,
    pub error: Error,
}

/// Analyses user apps and updated system apps of user 0. `tmp` is a local scratch directory;
/// `helper_dex` the on-device helper for names and icons (skipped when `None`).
pub async fn analyze(
    device: &Device,
    sdk: u32,
    tmp: &Path,
    helper_dex: Option<&Path>,
    mut on_update: impl FnMut(&Analysis),
) -> Result<Analysis> {
    let mut a = Analysis { stage: AnalysisStage::Packages, apps: vec![], accounts: vec![], other: None, errors: vec![] };
    on_update(&a);

    a.apps = list_apps(device).await?;
    // The helper stays on the phone from the labels to the Resto counts, then is removed.
    let mut helper = None;
    if let Some(dex) = helper_dex {
        a.stage = AnalysisStage::Labels;
        on_update(&a);
        match crate::helper::Helper::deploy(device, dex, tmp).await {
            Ok(h) => {
                if let Err(error) = add_labels(&h, &mut a.apps).await {
                    a.errors.push(StageError { stage: AnalysisStage::Labels, error });
                }
                helper = Some(h);
            }
            Err(error) => a.errors.push(StageError { stage: AnalysisStage::Labels, error }),
        }
    }
    a.stage = AnalysisStage::Sizes;
    on_update(&a);

    if let Err(error) = add_sizes(device, &mut a.apps).await {
        a.errors.push(StageError { stage: AnalysisStage::Sizes, error });
    }
    a.stage = AnalysisStage::Permissions;
    on_update(&a);

    match add_permissions(device, &mut a.apps).await {
        Ok(accounts) => a.accounts = accounts,
        Err(error) => a.errors.push(StageError { stage: AnalysisStage::Permissions, error }),
    }
    a.stage = AnalysisStage::Keystore;
    on_update(&a);

    match keystore::keystore_uids(device, sdk, &tmp.join("keystore")).await {
        Ok(uids) => mark_keystore(&mut a.apps, &uids),
        Err(error) => a.errors.push(StageError { stage: AnalysisStage::Keystore, error }),
    }
    a.stage = AnalysisStage::Other;
    on_update(&a);

    match crate::other::analyze(device, sdk, helper.as_ref()).await {
        Ok(o) => a.other = Some(o),
        Err(error) => a.errors.push(StageError { stage: AnalysisStage::Other, error }),
    }
    if let Some(h) = helper {
        let _ = h.remove().await;
    }
    a.stage = AnalysisStage::Done;
    on_update(&a);
    Ok(a)
}

async fn list_apps(device: &Device) -> Result<Vec<AppInfo>> {
    // All packages installed for user 0 (`-U` adds the UID).
    let users = device.shell("pm list packages -U").await?.ok()?.stdout_str();
    let uids: HashMap<String, u32> = packages::parse_pm_list_uids(&users).into_iter().collect();
    let dump = device.shell("dumpsys package packages").await?.ok()?.stdout_str();
    Ok(select_apps(packages::parse_packages(&dump, 0), &uids))
}

/// User apps, plus system apps updated with their APK in `/data/app` (those are complete APKs;
/// the ones under `/system` are not).
fn select_apps(dumps: Vec<packages::PackageDump>, uids: &HashMap<String, u32>) -> Vec<AppInfo> {
    let mut apps: Vec<AppInfo> = dumps
        .into_iter()
        .filter_map(|p| {
            let uid = *uids.get(&p.package)?;
            let kind = if !p.is_system() {
                AppKind::User
            } else if p.is_updated_system() && p.code_path.starts_with("/data/app/") {
                AppKind::UpdatedSystem
            } else {
                return None;
            };
            let exclusion = match kind {
                AppKind::UpdatedSystem => exclusion(&p.package, p.shared_user.as_deref()),
                AppKind::User => None,
            };
            Some(AppInfo {
                kind,
                exclusion,
                label: None,
                icon: None,
                uid,
                version_code: p.version_code,
                version_name: p.version_name,
                min_sdk: p.min_sdk,
                target_sdk: p.target_sdk,
                code_path: p.code_path,
                apks: vec![],
                splits: p.splits,
                primary_abi: p.primary_abi,
                installer: p.installer,
                signatures: p.signatures,
                shared_user: p.shared_user,
                flags: p.pkg_flags,
                data: DataSizes::default(),
                permissions: Permissions { runtime: p.runtime_granted, ..Default::default() },
                keystore: false,
                package: p.package,
            })
        })
        .collect();
    apps.sort_by(|a, b| a.package.cmp(&b.package));
    apps
}

async fn add_labels(helper: &crate::helper::Helper<'_>, apps: &mut [AppInfo]) -> Result<()> {
    let packages: Vec<String> = apps.iter().map(|a| a.package.clone()).collect();
    let mut labels = helper.labels(&packages).await?;
    for app in apps.iter_mut() {
        if let Some(l) = labels.remove(&app.package) {
            app.label = l.label.filter(|l| !l.is_empty());
            app.icon = l.icon;
        }
    }
    Ok(())
}

/// Data locations of a package for user 0, as (part, path).
pub fn data_paths(package: &str) -> [(&'static str, String); 5] {
    [
        ("ce", format!("/data/data/{package}")),
        ("de", format!("/data/user_de/0/{package}")),
        ("extData", format!("/data/media/0/Android/data/{package}")),
        ("obb", format!("/data/media/0/Android/obb/{package}")),
        ("media", format!("/data/media/0/Android/media/{package}")),
    ]
}

/// Apps per root shell call: one script for every app is ~100 KB, which `adb shell` rejects
/// (`error: closed`, no output).
const SIZE_CHUNK: usize = 20;
/// Root shell calls in flight at once; `du` is IO-bound on the phone, so a few overlap well.
const SIZE_PARALLEL: usize = 4;

/// APK files and data sizes, in root shell calls of [`SIZE_CHUNK`] apps, [`SIZE_PARALLEL`] at a time.
///
/// Each call runs one `du` for every data location and one for the subdirectories a backup leaves
/// out, plus one `stat` for the APKs (a handful of processes per chunk, not per app). The output
/// is matched back to apps by path.
async fn add_sizes(device: &Device, apps: &mut [AppInfo]) -> Result<()> {
    let scripts: Vec<String> = apps
        .chunks(SIZE_CHUNK)
        .map(|chunk| {
            let (mut tops, mut subs, mut codes) = (String::new(), String::new(), String::new());
            for app in chunk {
                for ((_, path), kind) in data_paths(&app.package).into_iter().zip(DataPart::ALL) {
                    tops.push_str(&format!(" {}", shell_quote(&path)));
                    for dir in kind.excludes(true) {
                        subs.push_str(&format!(" {}", shell_quote(&format!("{path}/{dir}"))));
                    }
                }
                codes.push_str(&format!(" {}/*.apk", shell_quote(app.code_path.trim_end_matches('/'))));
            }
            // Missing paths only print errors; `exit 0` so only adb's own failure counts.
            format!("du -sk{tops} 2>/dev/null; echo @@sub; du -sk{subs} 2>/dev/null; echo @@apk; stat -c '%s %n'{codes} 2>/dev/null; exit 0")
        })
        .collect();

    let mut outputs: Vec<String> = vec![String::new(); scripts.len()];
    let mut next = scripts.iter().enumerate();
    let mut set = tokio::task::JoinSet::new();
    loop {
        while set.len() < SIZE_PARALLEL {
            let Some((i, script)) = next.next() else { break };
            let (device, script) = (device.clone(), script.clone());
            set.spawn(async move { (i, device.root_shell(&script).await.and_then(|o| o.ok()).map(|o| o.stdout_str())) });
        }
        let Some(done) = set.join_next().await else { break };
        let (i, out) = done.map_err(|e| Error::UnexpectedOutput { what: format!("size task: {e}") })?;
        outputs[i] = out?;
    }

    let mut by_path: HashMap<String, (usize, &'static str, Option<&'static str>)> = HashMap::new();
    let mut by_code: HashMap<String, usize> = HashMap::new();
    for (i, app) in apps.iter().enumerate() {
        for ((part, path), kind) in data_paths(&app.package).into_iter().zip(DataPart::ALL) {
            for dir in kind.excludes(true) {
                by_path.insert(format!("{path}/{dir}"), (i, part, Some(dir)));
            }
            by_path.insert(path, (i, part, None));
        }
        by_code.insert(app.code_path.trim_end_matches('/').to_string(), i);
    }

    for out in &outputs {
        let mut section = 0;
        for line in out.lines() {
            match line.trim() {
                "@@sub" => { section = 1; continue; }
                "@@apk" => { section = 2; continue; }
                _ => {}
            }
            let Some((num, path)) = line.split_once(|c: char| c.is_whitespace()) else { continue };
            let path = path.trim();
            let Ok(n) = num.parse::<u64>() else { continue };
            if section == 2 {
                let Some((dir, name)) = path.rsplit_once('/') else { continue };
                if let Some(&i) = by_code.get(dir) {
                    apps[i].apks.push(ApkFile { name: name.to_string(), size: n });
                }
                continue;
            }
            let Some(&(i, part, dir)) = by_path.get(path) else { continue };
            let app = &mut apps[i];
            let bytes = n * 1024;
            match dir {
                None => match part {
                    "ce" => app.data.ce = Some(bytes),
                    "de" => app.data.de = Some(bytes),
                    "extData" => app.data.ext_data = Some(bytes),
                    "obb" => app.data.obb = Some(bytes),
                    "media" => app.data.media = Some(bytes),
                    _ => {}
                },
                Some(dir) => {
                    // Sections arrive top-level first, so the total exists by now.
                    let total = match part {
                        "ce" => &mut app.data.ce,
                        "de" => &mut app.data.de,
                        "extData" => &mut app.data.ext_data,
                        _ => continue,
                    };
                    *total = total.map(|t| t.saturating_sub(bytes));
                    if dir == "no_backup" {
                        match part {
                            "ce" => app.data.ce_no_backup = bytes,
                            "de" => app.data.de_no_backup = bytes,
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// AppOps, special access and roles, plus the account list; one shell call.
async fn add_permissions(device: &Device, apps: &mut [AppInfo]) -> Result<Vec<Account>> {
    let mut script = String::new();
    for r in access::ROLES {
        script.push_str(&format!("echo @@role.android.app.role.{r}; cmd role get-role-holders android.app.role.{r}; "));
    }
    script.push_str("echo @@secure; for k in enabled_notification_listeners enabled_accessibility_services default_input_method enabled_input_methods; do echo \"$k=$(settings get secure $k)\"; done; ");
    script.push_str("echo @@deviceidle; dumpsys deviceidle whitelist; echo @@dpm; dumpsys device_policy; echo @@accounts; dumpsys account; ");
    for app in apps.iter() {
        script.push_str(&format!("echo {}; appops get {}; ", shell_quote(&format!("@@appops.{}", app.package)), shell_quote(&app.package)));
    }
    let out = device.shell(&script).await?.stdout_str();
    let sections = split_sections(&out);
    let get = |k: &str| sections.get(k).map(String::as_str).unwrap_or_default();

    let mut roles: HashMap<String, Vec<String>> = HashMap::new();
    for (name, body) in &sections {
        if let Some(role) = name.strip_prefix("role.") {
            for pkg in body.lines().map(str::trim).filter(|l| !l.is_empty()) {
                roles.entry(pkg.to_string()).or_default().push(role.to_string());
            }
        }
    }
    let secure = access::parse_key_values(get("secure"));
    let mut special = access::parse_special_access(&secure, get("deviceidle"), get("dpm"));
    for app in apps.iter_mut() {
        app.permissions.app_ops = access::parse_appops(get(&format!("appops.{}", app.package)));
        app.permissions.special = special.remove(&app.package).unwrap_or_default();
        let mut r = roles.remove(&app.package).unwrap_or_default();
        r.sort();
        app.permissions.roles = r;
    }
    Ok(access::parse_accounts(get("accounts")))
}

fn mark_keystore(apps: &mut [AppInfo], uids: &BTreeSet<u32>) {
    for app in apps {
        app.keystore = uids.contains(&app.uid);
    }
}

/// Splits output on `@@name` marker lines.
pub(crate) fn split_sections(out: &str) -> HashMap<String, String> {
    let mut sections = HashMap::new();
    let mut current: Option<(String, String)> = None;
    for line in out.lines() {
        if let Some(name) = line.strip_prefix("@@") {
            if let Some((n, body)) = current.take() {
                sections.insert(n, body);
            }
            current = Some((name.trim().to_string(), String::new()));
        } else if let Some((_, body)) = current.as_mut() {
            body.push_str(line);
            body.push('\n');
        }
    }
    if let Some((n, body)) = current {
        sections.insert(n, body);
    }
    sections
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusions() {
        assert_eq!(exclusion("com.google.android.gms", None), Some(Exclusion::FixedList));
        assert_eq!(exclusion("com.google.android.trichromelibrary_699813532", None), Some(Exclusion::FixedList));
        assert_eq!(exclusion("com.google.android.as.oss", None), Some(Exclusion::FixedList));
        assert_eq!(exclusion("com.google.android.euicc", None), Some(Exclusion::FixedList));
        assert_eq!(
            exclusion("com.android.phone", Some("android.uid.phone")),
            Some(Exclusion::SystemSharedUser { shared_user: "android.uid.phone".into() })
        );
        assert_eq!(exclusion("com.android.chrome", None), None);
        assert_eq!(exclusion("com.google.android.gmscore", None), None);
    }

    #[test]
    fn selects_user_and_updated_system_apps_from_pixel() {
        let out = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/pixel10pro-a16/dumpsys-package-packages.txt"
        ))
        .unwrap();
        let uids: HashMap<String, u32> = ["com.termux", "com.example.app", "com.aurora.store", "com.android.chrome", "com.android.phone"]
            .iter()
            .enumerate()
            .map(|(i, p)| (p.to_string(), 10000 + i as u32))
            .collect();
        let apps = select_apps(packages::parse_packages(&out, 0), &uids);
        let kinds: Vec<(&str, AppKind)> = apps.iter().map(|a| (a.package.as_str(), a.kind)).collect();
        // com.android.phone is a system app that is not updated in /data/app: left out.
        assert_eq!(
            kinds,
            [
                ("com.android.chrome", AppKind::UpdatedSystem),
                ("com.aurora.store", AppKind::User),
                ("com.example.app", AppKind::User),
                ("com.termux", AppKind::User),
            ]
        );
        assert!(apps.iter().all(|a| a.exclusion.is_none()));
    }
}
