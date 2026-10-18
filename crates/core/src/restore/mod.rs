//! Restoring apps from a backup folder onto the connected phone (ui-spec §4, backup-android.md
//! point 2 restore order and point 3).

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::analysis::{AppInfo, Permissions, SpecialAccess};
use crate::backup::{ItemStatus, JobEvent, JobResult, filter_permissions};
use crate::compat::{AppCompat, PlannedAction};
use crate::device::Device;
use crate::format::{DataPart, ItemError, Manifest};
use crate::root::shell_quote;
use crate::stream::file_to_device;
use crate::{Error, Result};

/// Scratch directory on the phone, writable by the adb shell user.
pub const REMOTE_TMP: &str = "/data/local/tmp/adbkeep";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RestoreSelection {
    pub package: String,
    /// Install or update the APK when the plan says so.
    pub app: bool,
    pub data: Vec<DataPart>,
    pub permissions: bool,
    #[serde(default)]
    pub excluded_permissions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RestoreRequest {
    pub backup_path: String,
    pub apps: Vec<RestoreSelection>,
    /// `-i com.android.vending`: the app shows as installed from Play.
    pub install_as_play: bool,
    /// `--bypass-low-target-sdk-block` (not verified).
    pub bypass_low_target_sdk: bool,
    /// Resto tab.
    #[serde(default)]
    pub other: crate::other::restore::OtherRestoreSelection,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RestoredApp {
    pub package: String,
    /// UID on the target after installing (shown in advanced mode).
    pub uid: Option<u32>,
    pub installed: bool,
    pub data_parts: Vec<DataPart>,
    /// Permission keys (see `backup::permission_keys`) that could not be applied.
    pub failed_permissions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum RestoreStepKind {
    Install { apks: Vec<String> },
    ForceStop,
    Data { part: DataPart, archive: String },
    Permission { key: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RestoreStep {
    pub package: String,
    #[serde(flatten)]
    pub kind: RestoreStepKind,
    /// Commands shown when simulating; data steps run the second one as root.
    pub commands: Vec<String>,
    /// Bytes read from the backup, for progress.
    pub estimate: u64,
}

impl RestoreStep {
    fn label(&self) -> String {
        match &self.kind {
            RestoreStepKind::Install { .. } => "install".into(),
            RestoreStepKind::ForceStop => "forceStop".into(),
            RestoreStepKind::Data { part, .. } => format!("data:{}", serde_json::to_value(part).unwrap().as_str().unwrap()),
            RestoreStepKind::Permission { key } => format!("permission:{key}"),
        }
    }
}

/// Owner group of each data location besides the app UID (backup-android.md point 2).
fn group_for(part: DataPart) -> Option<u32> {
    match part {
        DataPart::Ce | DataPart::De => None,
        DataPart::ExtData => Some(1078), // ext_data_rw
        DataPart::Obb => Some(1079),     // ext_obb_rw
        DataPart::Media => Some(1023),   // media_rw
    }
}

pub fn remote_archive(pkg: &str, part: DataPart) -> String {
    format!("{REMOTE_TMP}/{pkg}.{}.tar", serde_json::to_value(part).unwrap().as_str().unwrap())
}

/// Root script: wipe the old data (keeping `lib` and the caches the install created), extract,
/// give everything to the target's UID, fix the cache group and SELinux labels, and print the UID.
pub fn data_script(pkg: &str, part: DataPart) -> String {
    let dir = format!("{}/{pkg}", part.base_dir());
    let q = shell_quote;
    let group = match group_for(part) {
        Some(g) => g.to_string(),
        None => "$uid".into(),
    };
    let caches = match part {
        // The cache gid is the UID + 10000 (u0_a123_cache = 20123).
        DataPart::Ce | DataPart::De => format!(
            "for c in cache code_cache; do [ -d {d}/$c ] && chown -R $uid:$((uid + 10000)) {d}/$c; done; ",
            d = q(&dir)
        ),
        _ => String::new(),
    };
    format!(
        "set -e; uid=$(stat -c %u {ce}); mkdir -p {d}; \
         find {d} -mindepth 1 -maxdepth 1 ! -name lib ! -name cache ! -name code_cache -exec rm -rf {{}} +; \
         tar -xf {tar} -C {base}; chown -R $uid:{group} {d}; {caches}restorecon -RFD {d}; echo uid=$uid",
        ce = q(&format!("/data/data/{pkg}")),
        d = q(&dir),
        tar = q(&remote_archive(pkg, part)),
        base = part.base_dir(),
    )
}

/// Shell (not root) commands applying one permission-like item.
pub fn permission_command(pkg: &str, key_item: &PermissionItem) -> String {
    let p = shell_quote(pkg);
    match key_item {
        PermissionItem::Runtime(perm) => format!("pm grant {p} {}", shell_quote(perm)),
        PermissionItem::Op(op, mode) => format!("appops set {p} {} {}", shell_quote(op), shell_quote(mode)),
        PermissionItem::Role(role) => format!("cmd role add-role-holder {} {p}", shell_quote(role)),
        PermissionItem::Special(s) => match s {
            SpecialAccess::NotificationListener { component } => format!("cmd notification allow_listener {}", shell_quote(component)),
            SpecialAccess::Accessibility { component } => {
                let c = shell_quote(component);
                format!(
                    "cur=$(settings get secure enabled_accessibility_services); \
                     case \":$cur:\" in *:{c}:*) ;; *) if [ -z \"$cur\" ] || [ \"$cur\" = null ]; then n={c}; else n=\"$cur:\"{c}; fi; \
                     settings put secure enabled_accessibility_services \"$n\"; settings put secure accessibility_enabled 1;; esac"
                )
            }
            SpecialAccess::InputMethod { component, default } => {
                let c = shell_quote(component);
                if *default { format!("ime enable {c} && ime set {c}") } else { format!("ime enable {c}") }
            }
            SpecialAccess::BatteryUnrestricted => format!("dumpsys deviceidle whitelist +{p}"),
            SpecialAccess::DeviceAdmin { component } => format!("dpm set-active-admin --user 0 {}", shell_quote(component)),
        },
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PermissionItem {
    Runtime(String),
    Op(String, String),
    Special(SpecialAccess),
    Role(String),
}

/// Items with the same keys as `backup::permission_keys`.
pub fn permission_items(p: &Permissions) -> Vec<(String, PermissionItem)> {
    let mut v: Vec<(String, PermissionItem)> = p.runtime.iter().map(|r| (format!("runtime:{r}"), PermissionItem::Runtime(r.clone()))).collect();
    v.extend(p.app_ops.iter().map(|o| (format!("op:{}", o.op), PermissionItem::Op(o.op.clone(), o.mode.clone()))));
    v.extend(p.special.iter().map(|s| (crate::backup::special_key(s), PermissionItem::Special(s.clone()))));
    v.extend(p.roles.iter().map(|r| (format!("role:{r}"), PermissionItem::Role(r.clone()))));
    v
}

/// Steps for the selection, following each app's compatibility verdict.
pub fn plan(
    manifest: &Manifest,
    metas: &HashMap<String, AppInfo>,
    compat: &[AppCompat],
    req: &RestoreRequest,
    file_sizes: &HashMap<String, u64>,
) -> Vec<RestoreStep> {
    let compat: HashMap<&str, &AppCompat> = compat.iter().map(|c| (c.package.as_str(), c)).collect();
    let entries: HashMap<&str, _> = manifest.apps.iter().map(|a| (a.package.as_str(), a)).collect();
    let mut steps = Vec::new();
    for sel in &req.apps {
        let (Some(entry), Some(c), Some(meta)) =
            (entries.get(sel.package.as_str()), compat.get(sel.package.as_str()), metas.get(&sel.package))
        else {
            continue;
        };
        let pkg = &sel.package;
        if !valid_package(pkg) || entry.apks.iter().chain(entry.data.iter().map(|d| &d.path)).any(|p| !valid_rel(p)) {
            continue;
        }
        let install = sel.app && matches!(c.action, PlannedAction::Install | PlannedAction::Update);
        // Skipped, or not installed and not to be installed: nothing to restore into.
        if c.action == PlannedAction::Skip || (!sel.app && c.action == PlannedAction::Install) {
            continue;
        }
        if install {
            let mut cmd = vec!["install-multiple".to_string(), "-r".into()];
            if req.install_as_play {
                cmd.extend(["-i".into(), "com.android.vending".into()]);
            }
            if req.bypass_low_target_sdk {
                cmd.push("--bypass-low-target-sdk-block".into());
            }
            cmd.extend(entry.apks.iter().cloned());
            steps.push(RestoreStep {
                package: pkg.clone(),
                kind: RestoreStepKind::Install { apks: entry.apks.clone() },
                commands: vec![format!("adb {}", cmd.iter().map(|a| shell_quote(a)).collect::<Vec<_>>().join(" "))],
                estimate: entry.apks.iter().map(|a| file_sizes.get(a).copied().unwrap_or(0)).sum(),
            });
        }
        // Without installing, data only goes onto an install of the same or a newer version.
        let parts: Vec<_> = if c.data_allowed && (install || c.action == PlannedAction::DataOnly) {
            entry.data.iter().filter(|d| sel.data.contains(&d.part)).collect()
        } else {
            vec![]
        };
        if !parts.is_empty() {
            steps.push(RestoreStep {
                package: pkg.clone(),
                kind: RestoreStepKind::ForceStop,
                commands: vec![format!("am force-stop {}", shell_quote(pkg))],
                estimate: 0,
            });
        }
        for d in parts {
            steps.push(RestoreStep {
                package: pkg.clone(),
                kind: RestoreStepKind::Data { part: d.part, archive: d.path.clone() },
                commands: vec![
                    format!("zstd -dc {} | adb shell -T 'cat > {}'", shell_quote(&d.path), remote_archive(pkg, d.part)),
                    data_script(pkg, d.part),
                ],
                estimate: file_sizes.get(&d.path).copied().unwrap_or(0),
            });
        }
        if sel.permissions && entry.permissions {
            let perms = filter_permissions(&meta.permissions, &sel.excluded_permissions);
            for (key, item) in permission_items(&perms) {
                steps.push(RestoreStep {
                    package: pkg.clone(),
                    commands: vec![permission_command(pkg, &item)],
                    kind: RestoreStepKind::Permission { key },
                    estimate: 0,
                });
            }
        }
    }
    steps
}

/// Size and SHA-256 of a backup file against the manifest, before anything on the phone is touched.
async fn check_archive(dir: &Path, manifest: &Manifest, rel: &str) -> Result<()> {
    let Some(entry) = manifest.files.iter().find(|f| f.path == rel) else { return Ok(()) };
    let (path, size, sha) = (dir.join(rel), entry.size, entry.sha256.clone());
    let ok = tokio::task::spawn_blocking(move || {
        Ok::<_, Error>(std::fs::metadata(&path)?.len() == size && crate::stream::sha256_file(&path)? == sha)
    })
    .await
    .map_err(|e| Error::Io { reason: e.to_string() })??;
    if ok { Ok(()) } else { Err(Error::Io { reason: format!("{rel}: does not match the manifest (corrupt backup file)") }) }
}

/// Android package names: letters, digits, `_` and `.`; this also keeps them out of shell
/// and path tricks when they come from a backup file.
pub fn valid_package(pkg: &str) -> bool {
    !pkg.is_empty() && !pkg.starts_with('.') && pkg.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.') && !pkg.contains("..")
}

/// Relative path inside the backup, without `..` or a leading `/`.
fn valid_rel(p: &str) -> bool {
    !p.starts_with('/') && !p.split('/').any(|c| c == ".." || c.is_empty())
}

const PROGRESS_EVERY: Duration = Duration::from_millis(200);

/// Runs the plan. `dir` is the (unpacked) backup folder. Like the backup, single failures are
/// recorded and the rest goes on; cancelling or losing the phone stops it.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    device: &Device,
    sdk: u32,
    dir: &Path,
    manifest: &Manifest,
    steps: &[RestoreStep],
    req: &RestoreRequest,
    extras: &crate::backup::Extras<'_>,
    cancel: &AtomicBool,
    mut on_event: impl FnMut(JobEvent),
) -> Result<JobResult> {
    device.root().ok_or(Error::RootUnavailable)?;
    let start = Instant::now();
    let device = &device.tagged("restore");
    let mut packages: Vec<String> = Vec::new();
    for s in steps {
        if !packages.contains(&s.package) {
            packages.push(s.package.clone());
        }
    }
    let other_items: Vec<crate::other::OtherItem> =
        req.other.items().into_iter().filter(|i| manifest.other.iter().any(|e| e.item == *i)).collect();
    let other_sizes: u64 = manifest
        .other
        .iter()
        .filter(|e| other_items.contains(&e.item))
        .flat_map(|e| e.files.iter())
        .filter_map(|f| manifest.files.iter().find(|x| &x.path == f).map(|x| x.size))
        .sum();
    let total_bytes: u64 = steps.iter().map(|s| s.estimate).sum::<u64>() + other_sizes;
    let mut keys = packages.clone();
    keys.extend(other_items.iter().map(|i| crate::backup::other_key(*i)));
    on_event(JobEvent::Started { total_bytes, packages: keys });
    device.shell(&format!("mkdir -p {REMOTE_TMP}")).await?.ok()?;

    let mut errors: Vec<ItemError> = Vec::new();
    let mut restored: Vec<RestoredApp> = Vec::new();
    let mut bytes_done = 0u64;
    let mut stopped: Option<Error> = None;
    let mut last_emit = Instant::now() - PROGRESS_EVERY;

    'apps: for pkg in &packages {
        on_event(JobEvent::Item { package: pkg.clone(), status: ItemStatus::Running });
        let errors_before = errors.len();
        let mut app = RestoredApp { package: pkg.clone(), uid: None, installed: false, data_parts: vec![], failed_permissions: vec![] };
        for step in steps.iter().filter(|s| &s.package == pkg) {
            if cancel.load(Ordering::Relaxed) {
                stopped = Some(Error::Cancelled);
                break 'apps;
            }
            let base = bytes_done;
            let label = step.label();
            let mut attempt = 0;
            // Every app step can be repeated safely: install -r, wipe-and-extract, grants.
            let result: Result<()> = loop {
                if let Err(e) = crate::pause::wait_ready(extras.gate, device.serial(), cancel, &mut on_event).await {
                    break Err(e);
                }
                let r: Result<()> = match &step.kind {
                RestoreStepKind::Install { apks } => {
                    let mut args: Vec<String> = vec!["install-multiple".into(), "-r".into()];
                    if req.install_as_play {
                        args.extend(["-i".into(), "com.android.vending".into()]);
                    }
                    if req.bypass_low_target_sdk {
                        args.push("--bypass-low-target-sdk-block".into());
                    }
                    args.extend(apks.iter().map(|a| dir.join(a).display().to_string()));
                    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
                    on_event(JobEvent::Progress { bytes_done, total_bytes, package: pkg.clone(), step: label.clone(), speed: 0 });
                    match device.adb_run(&refs, None).await {
                        // `adb install` can exit 0 and still print "Failure [...]".
                        Ok(out) if out.success() && !out.stdout_str().contains("Failure") => {
                            app.installed = true;
                            Ok(())
                        }
                        Ok(out) => Err(Error::CommandFailed {
                            exit_code: out.exit_code,
                            stderr: format!("{}{}", out.stdout_str().trim(), out.stderr.trim()),
                        }),
                        Err(e) => Err(e),
                    }
                }
                RestoreStepKind::ForceStop => device.shell(&step.commands[0]).await.and_then(|o| o.ok().map(|_| ())),
                RestoreStepKind::Data { part, archive } => {
                    let remote = remote_archive(pkg, *part);
                    // The old data is wiped once the new is on the phone: make sure it is intact first.
                    let intact = check_archive(dir, manifest, archive).await;
                    let pushed = if let Err(e) = intact { Err(e) } else { file_to_device(device, &dir.join(archive), true, &remote, cancel, |n| {
                        if last_emit.elapsed() >= PROGRESS_EVERY {
                            on_event(JobEvent::Progress {
                                bytes_done: base + n,
                                total_bytes,
                                package: pkg.clone(),
                                step: label.clone(),
                                speed: 0,
                            });
                            last_emit = Instant::now();
                        }
                    })
                    .await };
                    match pushed {
                        Ok(_) => {
                            let out = device.root_shell(&step.commands[1]).await;
                            let _ = device.shell(&format!("rm -f {}", shell_quote(&remote))).await;
                            match out.and_then(|o| o.ok()) {
                                Ok(o) => {
                                    app.uid = o.stdout_str().lines().find_map(|l| l.trim().strip_prefix("uid=")?.parse().ok());
                                    app.data_parts.push(*part);
                                    Ok(())
                                }
                                Err(e) => Err(e),
                            }
                        }
                        Err(e) => Err(e),
                    }
                }
                RestoreStepKind::Permission { .. } => device.shell(&step.commands[0]).await.and_then(|o| o.ok().map(|_| ())),
                };
                match r {
                    Err(e) if crate::pause::should_retry(extras.gate, device, &e, attempt).await => attempt += 1,
                    r => break r,
                }
            };
            bytes_done = bytes_done.max(base + step.estimate);
            if let Err(error) = result {
                if let RestoreStepKind::Permission { key } = &step.kind {
                    app.failed_permissions.push(key.clone());
                }
                let fatal = matches!(error, Error::Cancelled | Error::DeviceUnavailable { .. });
                errors.push(ItemError { package: Some(pkg.clone()), step: label, error: error.clone() });
                if fatal {
                    stopped = Some(error);
                    break 'apps;
                }
                // Without the app installed nothing else of it can be restored.
                if matches!(step.kind, RestoreStepKind::Install { .. }) {
                    break;
                }
            }
        }
        if app.uid.is_none()
            && let Ok(o) = device.shell(&format!("pm list packages -U {}", shell_quote(pkg))).await
        {
            app.uid = crate::analysis::packages::parse_pm_list_uids(&o.stdout_str())
                .into_iter()
                .find(|(p, _)| p == pkg)
                .map(|(_, u)| u);
        }
        restored.push(app);
        let status = if errors.len() > errors_before { ItemStatus::Error } else { ItemStatus::Done };
        on_event(JobEvent::Item { package: pkg.clone(), status });
    }
    // Resto tab, with the helper deployed once if needed.
    let mut other_outcomes = Vec::new();
    if stopped.is_none() && !other_items.is_empty() {
        let needs_helper = other_items.iter().any(|i| i.needs_helper() && !(*i == crate::other::OtherItem::Contacts && req.other.contacts_as_vcf));
        let helper = match (needs_helper, extras.helper_dex) {
            (true, Some(dex)) => crate::helper::Helper::deploy(device, dex, extras.tmp).await.ok(),
            _ => None,
        };
        let job = crate::other::restore::OtherRestoreJob { device, sdk, dir, manifest, cancel, helper: helper.as_ref() };
        for item in &other_items {
            if cancel.load(Ordering::Relaxed) {
                stopped = Some(Error::Cancelled);
                break;
            }
            let key = crate::backup::other_key(*item);
            on_event(JobEvent::Item { package: key.clone(), status: ItemStatus::Running });
            on_event(JobEvent::Progress { bytes_done, total_bytes, package: key.clone(), step: key.clone(), speed: 0 });
            // Not retried after a disconnection: inserting rows again could duplicate them.
            let result = match crate::pause::wait_ready(extras.gate, device.serial(), cancel, &mut on_event).await {
                Ok(()) => job.run(*item, &req.other).await,
                Err(e) => Err(e),
            };
            let status = match result {
                Ok(outcome) => {
                    let partial = outcome.failed > 0;
                    if partial {
                        errors.push(crate::other::backup::failure(
                            *item,
                            Error::CommandFailed { exit_code: None, stderr: outcome.errors.join("; ") },
                        ));
                    }
                    other_outcomes.push(outcome);
                    if partial { ItemStatus::Error } else { ItemStatus::Done }
                }
                Err(error) => {
                    // With a gate the next item waits for the phone instead.
                    let fatal = matches!(error, Error::Cancelled)
                        || (extras.gate.is_none() && matches!(error, Error::DeviceUnavailable { .. }));
                    errors.push(crate::other::backup::failure(*item, error.clone()));
                    if fatal {
                        stopped = Some(error);
                    }
                    ItemStatus::Error
                }
            };
            on_event(JobEvent::Item { package: key, status });
            if stopped.is_some() {
                break;
            }
        }
        if let Some(h) = helper {
            let _ = h.remove().await;
        }
    }
    bytes_done = bytes_done.max(total_bytes);
    let _ = device.shell(&format!("rm -rf {REMOTE_TMP}")).await;

    let result = JobResult {
        path: req.backup_path.clone(),
        cancelled: matches!(stopped, Some(Error::Cancelled)),
        errors,
        bytes: bytes_done,
        size_on_disk: 0,
        duration_ms: start.elapsed().as_millis() as u32,
        restored,
        other: other_outcomes,
    };
    on_event(JobEvent::Finished { result: result.clone() });
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Manifest, HashMap<String, AppInfo>) {
        use crate::analysis::{AppKind, AppOp, DataSizes};
        use crate::format::{DataArchive, FORMAT_VERSION, ManifestApp, SourceDevice};
        let meta = AppInfo {
            package: "com.x".into(),
            kind: AppKind::User,
            uid: 10100,
            version_code: 5,
            version_name: None,
            min_sdk: 24,
            target_sdk: 34,
            code_path: String::new(),
            apks: vec![],
            splits: vec![],
            primary_abi: None,
            installer: None,
            signatures: vec![],
            shared_user: None,
            flags: vec![],
            data: DataSizes::default(),
            permissions: Permissions {
                runtime: vec!["android.permission.CAMERA".into()],
                app_ops: vec![AppOp { op: "MANAGE_EXTERNAL_STORAGE".into(), mode: "allow".into() }],
                special: vec![],
                roles: vec![],
            },
            keystore: false,
            exclusion: None,
            label: None,
            icon: None,
        };
        let manifest = Manifest {
            format_version: FORMAT_VERSION,
            app_version: String::new(),
            created_at: String::new(),
            name: String::new(),
            source: SourceDevice {
                serial: String::new(),
                manufacturer: String::new(),
                model: String::new(),
                android_release: String::new(),
                sdk: 36,
                abis: vec![],
                build_type: String::new(),
                build_id: String::new(),
                root_method: None,
            },
            apps: vec![ManifestApp {
                package: "com.x".into(),
                label: None,
                kind: AppKind::User,
                version_code: 5,
                version_name: None,
                meta: "apps/com.x/meta.json".into(),
                apks: vec!["apps/com.x/apk/base.apk".into()],
                data: vec![
                    DataArchive { part: DataPart::Ce, path: "apps/com.x/data/ce.tar.zst".into() },
                    DataArchive { part: DataPart::De, path: "apps/com.x/data/de.tar.zst".into() },
                ],
                permissions: true,
            }],
            other: vec![],
            accounts: vec![],
            keystore_packages: vec![],
            files: vec![],
            errors: vec![],
            complete: true,
        };
        (manifest, HashMap::from([("com.x".to_string(), meta)]))
    }

    fn compat(action: PlannedAction, data_allowed: bool) -> AppCompat {
        AppCompat {
            package: "com.x".into(),
            status: crate::compat::CompatStatus::Ok,
            reasons: vec![],
            action,
            data_allowed,
            installed_version: None,
            installed_signatures: vec![],
        }
    }

    fn request(app: bool, data: Vec<DataPart>, excluded: Vec<String>) -> RestoreRequest {
        RestoreRequest {
            backup_path: String::new(),
            apps: vec![RestoreSelection { package: "com.x".into(), app, data, permissions: true, excluded_permissions: excluded }],
            install_as_play: true,
            bypass_low_target_sdk: false,
            other: Default::default(),
        }
    }

    #[test]
    fn plans_install_data_and_permissions_in_order() {
        let (m, metas) = fixture();
        let steps = plan(&m, &metas, &[compat(PlannedAction::Install, true)], &request(true, vec![DataPart::Ce], vec![]), &HashMap::new());
        let labels: Vec<String> = steps.iter().map(RestoreStep::label).collect();
        assert_eq!(
            labels,
            ["install", "forceStop", "data:ce", "permission:runtime:android.permission.CAMERA", "permission:op:MANAGE_EXTERNAL_STORAGE"]
        );
        assert_eq!(steps[0].commands[0], "adb install-multiple -r -i com.android.vending apps/com.x/apk/base.apk");
        let excluded = vec!["op:MANAGE_EXTERNAL_STORAGE".to_string()];
        let steps = plan(&m, &metas, &[compat(PlannedAction::Install, true)], &request(true, vec![], excluded), &HashMap::new());
        assert_eq!(steps.iter().map(RestoreStep::label).collect::<Vec<_>>(), ["install", "permission:runtime:android.permission.CAMERA"]);
    }

    #[test]
    fn skips_what_cannot_be_restored() {
        let (m, metas) = fixture();
        let all = || request(true, vec![DataPart::Ce, DataPart::De], vec![]);
        assert!(plan(&m, &metas, &[compat(PlannedAction::Skip, false)], &all(), &HashMap::new()).is_empty());
        // Not installed and "Restaurar app" unticked: nothing.
        assert!(plan(&m, &metas, &[compat(PlannedAction::Install, true)], &request(false, vec![DataPart::Ce], vec![]), &HashMap::new()).is_empty());
        // Older version installed and not updated: permissions only, no data.
        let steps = plan(&m, &metas, &[compat(PlannedAction::Update, true)], &request(false, vec![DataPart::Ce], vec![]), &HashMap::new());
        assert!(steps.iter().all(|s| matches!(s.kind, RestoreStepKind::Permission { .. })));
        // Same version installed: data without installing.
        let steps = plan(&m, &metas, &[compat(PlannedAction::DataOnly, true)], &all(), &HashMap::new());
        assert_eq!(steps.iter().filter(|s| matches!(s.kind, RestoreStepKind::Data { .. })).count(), 2);
        assert!(!steps.iter().any(|s| matches!(s.kind, RestoreStepKind::Install { .. })));
    }

    #[test]
    fn rejects_suspicious_names() {
        assert!(valid_package("com.termux"));
        assert!(!valid_package("../system"));
        assert!(!valid_package("com..x"));
        assert!(!valid_package("a b"));
        let (mut m, metas) = fixture();
        m.apps[0].apks = vec!["../../etc/passwd".into()];
        assert!(plan(&m, &metas, &[compat(PlannedAction::Install, true)], &request(true, vec![], vec![]), &HashMap::new()).is_empty());
    }

    #[test]
    fn data_script_for_ce_and_external() {
        let ce = data_script("com.termux", DataPart::Ce);
        assert_eq!(
            ce,
            "set -e; uid=$(stat -c %u /data/data/com.termux); mkdir -p /data/data/com.termux; \
             find /data/data/com.termux -mindepth 1 -maxdepth 1 ! -name lib ! -name cache ! -name code_cache -exec rm -rf {} +; \
             tar -xf /data/local/tmp/adbkeep/com.termux.ce.tar -C /data/data; chown -R $uid:$uid /data/data/com.termux; \
             for c in cache code_cache; do [ -d /data/data/com.termux/$c ] && chown -R $uid:$((uid + 10000)) /data/data/com.termux/$c; done; \
             restorecon -RFD /data/data/com.termux; echo uid=$uid"
        );
        let ext = data_script("com.x", DataPart::ExtData);
        assert!(ext.contains("chown -R $uid:1078 /data/media/0/Android/data/com.x;"));
        assert!(!ext.contains("code_cache; do"));
    }

    #[test]
    fn permission_commands() {
        let pkg = "com.x";
        assert_eq!(permission_command(pkg, &PermissionItem::Runtime("android.permission.CAMERA".into())), "pm grant com.x android.permission.CAMERA");
        assert_eq!(
            permission_command(pkg, &PermissionItem::Op("MANAGE_EXTERNAL_STORAGE".into(), "allow".into())),
            "appops set com.x MANAGE_EXTERNAL_STORAGE allow"
        );
        assert_eq!(
            permission_command(pkg, &PermissionItem::Role("android.app.role.BROWSER".into())),
            "cmd role add-role-holder android.app.role.BROWSER com.x"
        );
        assert_eq!(
            permission_command(pkg, &PermissionItem::Special(SpecialAccess::BatteryUnrestricted)),
            "dumpsys deviceidle whitelist +com.x"
        );
        assert_eq!(
            permission_command(
                pkg,
                &PermissionItem::Special(SpecialAccess::InputMethod { component: "com.x/.Ime".into(), default: true })
            ),
            "ime enable com.x/.Ime && ime set com.x/.Ime"
        );
    }

    #[test]
    fn accessibility_script_appends_once() {
        let cmd = permission_command("com.x", &PermissionItem::Special(SpecialAccess::Accessibility { component: "com.x/.Svc".into() }));
        let run = |cur: &str| {
            let script = cmd.replace(
                "$(settings get secure enabled_accessibility_services)",
                &format!("'{cur}'"),
            )
            .replace("settings put secure enabled_accessibility_services", "echo")
            .replace("settings put secure accessibility_enabled 1", "true");
            let out = std::process::Command::new("sh").arg("-c").arg(script).output().unwrap();
            String::from_utf8(out.stdout).unwrap().trim().to_string()
        };
        assert_eq!(run("null"), "com.x/.Svc");
        assert_eq!(run("a/.B"), "a/.B:com.x/.Svc");
        assert_eq!(run("a/.B:com.x/.Svc"), "");
    }
}

/// Everything the Restore screen shows before starting (ui-spec §4 header and tabs).
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RestorePreview {
    pub backup_path: String,
    pub manifest: Manifest,
    pub apps: Vec<PreviewApp>,
    /// Resto tab items of the backup and what can be restored of each.
    pub other: Vec<crate::other::restore::OtherCompat>,
    /// Settings ticked when the backup was made ("table/key").
    pub saved_settings: Vec<String>,
    /// Shared-storage folders in the backup ("" = loose files).
    pub saved_folders: Vec<String>,
    /// Current default-app holders on the target, role → packages.
    pub target_roles: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PreviewApp {
    pub meta: AppInfo,
    pub compat: AppCompat,
}

/// Bytes the restore needs free on the phone: the APKs, the app data once extracted (from the
/// sizes measured when backing up) plus the biggest data archive pushed alongside it, and the
/// Resto files.
pub fn phone_space_needed(preview: &RestorePreview, req: &RestoreRequest) -> u64 {
    let sizes = file_sizes(&preview.manifest);
    let mut total = 0u64;
    let mut biggest = 0u64;
    for sel in &req.apps {
        let Some(app) = preview.apps.iter().find(|a| a.meta.package == sel.package) else { continue };
        if sel.app && let Some(entry) = preview.manifest.apps.iter().find(|e| e.package == sel.package) {
            total += entry.apks.iter().map(|a| sizes.get(a).copied().unwrap_or(0)).sum::<u64>();
        }
        for part in &sel.data {
            let d = &app.meta.data;
            let size = match part {
                DataPart::Ce => d.ce,
                DataPart::De => d.de,
                DataPart::ExtData => d.ext_data,
                DataPart::Obb => d.obb,
                DataPart::Media => d.media,
            }
            .unwrap_or(0);
            total += size;
            biggest = biggest.max(size);
        }
    }
    let items = req.other.items();
    total += preview
        .manifest
        .other
        .iter()
        .filter(|e| items.contains(&e.item))
        .flat_map(|e| e.files.iter())
        .map(|f| sizes.get(f).copied().unwrap_or(0))
        .sum::<u64>();
    total + biggest
}

/// Reads the backup's app metadata and checks each app against the target.
pub async fn preview(
    device: &Device,
    target: &crate::device::probe::DeviceInfo,
    backup_path: &Path,
    opts: crate::compat::CompatOptions,
    helper: bool,
) -> Result<RestorePreview> {
    let path = backup_path.to_path_buf();
    let (manifest, metas) = tokio::task::spawn_blocking(move || read_metas(&path))
        .await
        .map_err(|e| Error::Io { reason: e.to_string() })??;
    let installed = crate::analysis::installed::installed(device).await?;
    let apps = manifest
        .apps
        .iter()
        .filter_map(|entry| {
            let meta = metas.get(&entry.package)?.clone();
            let compat = crate::compat::check(entry, &meta, target, &installed, opts);
            Some(PreviewApp { meta, compat })
        })
        .collect();
    let wifi = wifi_networks(backup_path, &manifest);
    let other = crate::other::restore::preview(&manifest, target.sdk, helper, &wifi);
    let saved_settings = crate::other::restore::saved_settings(backup_path, &manifest);
    let saved_folders = crate::other::restore::saved_folders(&manifest);
    Ok(RestorePreview {
        backup_path: backup_path.display().to_string(),
        manifest,
        apps,
        other,
        saved_settings,
        saved_folders,
        target_roles: installed.roles,
    })
}

/// Networks in the backup's Wi-Fi config, for the preview and the simulation.
pub fn wifi_networks(backup_path: &Path, manifest: &Manifest) -> Vec<crate::other::wifi::WifiNetwork> {
    let path = "other/wifi/WifiConfigStore.xml";
    if !manifest.files.iter().any(|f| f.path == path) {
        return vec![];
    }
    crate::library::read_file(backup_path, path, 4 * 1024 * 1024)
        .map(|b| crate::other::wifi::parse(&String::from_utf8_lossy(&b)))
        .unwrap_or_default()
}

/// The manifest and every `apps/<pkg>/meta.json` (folder or packed backup).
pub fn read_metas(path: &Path) -> Result<(Manifest, HashMap<String, AppInfo>)> {
    let manifest = crate::library::read_manifest(path)?;
    let mut metas = HashMap::new();
    for app in &manifest.apps {
        let bytes = crate::library::read_file(path, &app.meta, 4 * 1024 * 1024)?;
        let meta: AppInfo =
            serde_json::from_slice(&bytes).map_err(|e| Error::UnexpectedOutput { what: format!("{}: {e}", app.meta) })?;
        metas.insert(app.package.clone(), meta);
    }
    Ok((manifest, metas))
}

/// Sizes of the backup's files by relative path, for progress estimates.
pub fn file_sizes(manifest: &Manifest) -> HashMap<String, u64> {
    manifest.files.iter().map(|f| (f.path.clone(), f.size)).collect()
}
