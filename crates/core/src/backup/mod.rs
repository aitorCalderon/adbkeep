//! Backing up apps: planning the steps (also shown by "Sólo simular") and running them.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

use crate::analysis::{AppInfo, Permissions, SpecialAccess};
use crate::device::Device;
use crate::device::probe::DeviceInfo;
use crate::format::{
    DataArchive, DataPart, FileEntry, ItemError, MANIFEST, Manifest, ManifestApp, SourceDevice, FORMAT_VERSION, rel,
    write_json,
};
use crate::log::LogEntry;
use crate::root::{as_root_binary, shell_quote};
use crate::stream::{root_to_file, sha256_file};
use crate::{Error, Result};

/// What to save of one app (ui-spec §3.1 checkboxes).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AppSelection {
    pub package: String,
    pub apk: bool,
    /// Empty: no data.
    pub data: Vec<DataPart>,
    pub permissions: bool,
    /// Advanced mode: permission keys left out (see [`permission_key`]).
    #[serde(default)]
    pub excluded_permissions: Vec<String>,
}

/// Stable key of one permission-like item, for [`AppSelection::excluded_permissions`]:
/// `runtime:<perm>`, `op:<OP>`, `special:<json>` or `role:<role>`.
pub fn permission_keys(p: &Permissions) -> Vec<String> {
    let mut keys: Vec<String> = p.runtime.iter().map(|r| format!("runtime:{r}")).collect();
    keys.extend(p.app_ops.iter().map(|o| format!("op:{}", o.op)));
    keys.extend(p.special.iter().map(special_key));
    keys.extend(p.roles.iter().map(|r| format!("role:{r}")));
    keys
}

pub fn special_key(s: &SpecialAccess) -> String {
    format!("special:{}", serde_json::to_string(s).unwrap_or_default())
}

/// `p` without the excluded keys.
pub fn filter_permissions(p: &Permissions, excluded: &[String]) -> Permissions {
    let keep = |k: String| !excluded.contains(&k);
    Permissions {
        runtime: p.runtime.iter().filter(|r| keep(format!("runtime:{r}"))).cloned().collect(),
        app_ops: p.app_ops.iter().filter(|o| keep(format!("op:{}", o.op))).cloned().collect(),
        special: p.special.iter().filter(|s| keep(special_key(s))).cloned().collect(),
        roles: p.roles.iter().filter(|r| keep(format!("role:{r}"))).cloned().collect(),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupRequest {
    /// Parent folder; the backup goes in `<dest_dir>/<name>`.
    pub dest_dir: String,
    pub name: String,
    pub apps: Vec<AppSelection>,
    pub include_no_backup: bool,
    /// zstd level, 1–19.
    pub compression_level: i32,
    /// Pack into `<name>.adbkeep` and remove the folder.
    pub pack: bool,
    /// Resto tab.
    #[serde(default)]
    pub other: crate::other::backup::OtherSelection,
    /// Encrypt the pack with this password (only with `pack`). Never written to disk.
    #[serde(default)]
    pub password: Option<String>,
    /// Continue this incomplete backup folder instead of starting a new one; `dest_dir` and
    /// `name` are then ignored.
    #[serde(default)]
    pub resume: Option<String>,
}

/// The request a backup was started with (without the password), for resuming it.
pub const REQUEST: &str = "report/request.json";

/// The request saved in an incomplete backup folder.
pub fn saved_request(dir: &Path) -> Result<BackupRequest> {
    let text = std::fs::read_to_string(dir.join(REQUEST))?;
    serde_json::from_str(&text).map_err(|e| Error::UnexpectedOutput { what: format!("{REQUEST}: {e}") })
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum StepKind {
    Apk { name: String },
    ForceStop,
    Data { part: DataPart },
    Meta,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub package: String,
    #[serde(flatten)]
    pub kind: StepKind,
    /// Command run on the phone, if any (shown when simulating).
    pub command: Option<String>,
    /// Destination inside the backup.
    pub dest: Option<String>,
    /// Expected bytes from the phone, for progress.
    pub estimate: u64,
}

impl Step {
    fn label(&self) -> String {
        match &self.kind {
            StepKind::Apk { name } => format!("apk:{name}"),
            StepKind::ForceStop => "forceStop".into(),
            StepKind::Data { part } => format!("data:{}", serde_json::to_value(part).unwrap().as_str().unwrap()),
            StepKind::Meta => "meta".into(),
        }
    }
}

/// Steps for the selection. Data parts that do not exist on the phone are skipped.
pub fn plan(apps: &[AppInfo], req: &BackupRequest) -> Vec<Step> {
    let by_pkg: HashMap<&str, &AppInfo> = apps.iter().map(|a| (a.package.as_str(), a)).collect();
    let mut steps = Vec::new();
    for sel in &req.apps {
        let Some(app) = by_pkg.get(sel.package.as_str()) else { continue };
        let pkg = &app.package;
        if sel.apk {
            for apk in &app.apks {
                steps.push(Step {
                    package: pkg.clone(),
                    kind: StepKind::Apk { name: apk.name.clone() },
                    command: Some(format!("cat {}", shell_quote(&format!("{}/{}", app.code_path, apk.name)))),
                    dest: Some(rel(&["apps", pkg, "apk", &apk.name])),
                    estimate: apk.size,
                });
            }
        }
        let parts: Vec<(DataPart, u64)> = DataPart::ALL
            .into_iter()
            .filter(|p| sel.data.contains(p))
            .filter_map(|p| size_of(app, p, req.include_no_backup).map(|s| (p, s)))
            .collect();
        if !parts.is_empty() {
            steps.push(Step {
                package: pkg.clone(),
                kind: StepKind::ForceStop,
                command: Some(format!("am force-stop {}", shell_quote(pkg))),
                dest: None,
                estimate: 0,
            });
        }
        for (part, size) in parts {
            let excludes: String = part
                .excludes(req.include_no_backup)
                .iter()
                .map(|e| format!(" --exclude={}", shell_quote(&format!("{pkg}/{e}"))))
                .collect();
            // Toybox tar stops at sockets ("unknown file type '140000'"), and a socket in a
            // backup is useless anyway, so they are listed and excluded.
            let q = shell_quote(pkg);
            steps.push(Step {
                package: pkg.clone(),
                kind: StepKind::Data { part },
                command: Some(format!(
                    "cd {} && find {q} -type s | tar -cpf - -X /dev/stdin{excludes} {q}",
                    part.base_dir()
                )),
                dest: Some(rel(&["apps", pkg, "data", part.archive_name()])),
                estimate: size,
            });
        }
        steps.push(Step {
            package: pkg.clone(),
            kind: StepKind::Meta,
            command: None,
            dest: Some(rel(&["apps", pkg, "meta.json"])),
            estimate: 0,
        });
    }
    steps
}

/// Bytes the tar of `part` will hold: the analysis sizes already leave out caches and `lib`,
/// and `no_backup` is added back when it is included.
fn size_of(app: &AppInfo, part: DataPart, include_no_backup: bool) -> Option<u64> {
    let extra = |n: u64| if include_no_backup { n } else { 0 };
    match part {
        DataPart::Ce => app.data.ce.map(|s| s + extra(app.data.ce_no_backup)),
        DataPart::De => app.data.de.map(|s| s + extra(app.data.de_no_backup)),
        DataPart::ExtData => app.data.ext_data,
        DataPart::Obb => app.data.obb,
        DataPart::Media => app.data.media,
    }
}

/// Full adb command lines of the plan, for "Sólo simular".
pub fn simulate(device: &Device, steps: &[Step]) -> Vec<String> {
    let serial = shell_quote(device.serial());
    steps
        .iter()
        .filter_map(|s| {
            let cmd = s.command.as_ref()?;
            Some(match (&s.kind, device.root()) {
                (StepKind::ForceStop, _) => format!("adb -s {serial} shell {}", shell_quote(cmd)),
                (_, Some(m)) => format!(
                    "adb -s {serial} shell -T {} > {}",
                    shell_quote(&as_root_binary(m, cmd)),
                    shell_quote(s.dest.as_deref().unwrap_or("-"))
                ),
                (_, None) => format!("# {cmd}"),
            })
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ItemStatus {
    Pending,
    Running,
    Done,
    /// Finished with some failed steps.
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct JobResult {
    /// The backup folder, or the `.adbkeep` file when packed.
    pub path: String,
    pub cancelled: bool,
    pub errors: Vec<ItemError>,
    /// Bytes received from the phone.
    pub bytes: u64,
    /// Bytes on the PC.
    pub size_on_disk: u64,
    pub duration_ms: u32,
    /// Per-app outcome of a restore; empty for backups.
    pub restored: Vec<crate::restore::RestoredApp>,
    /// Per-item outcome of the Resto tab in a restore; empty for backups.
    pub other: Vec<crate::other::restore::OtherOutcome>,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum JobEvent {
    #[serde(rename_all = "camelCase")]
    Started { total_bytes: u64, packages: Vec<String> },
    Item { package: String, status: ItemStatus },
    #[serde(rename_all = "camelCase")]
    Progress {
        bytes_done: u64,
        total_bytes: u64,
        package: String,
        step: String,
        /// Bytes per second over the last update interval.
        speed: u64,
    },
    Packing,
    /// Waiting for the phone to come back or be unlocked; the step in progress is retried.
    Paused { reason: crate::pause::PauseReason },
    Resumed,
    Finished { result: JobResult },
    /// The job could not run or finish at all (e.g. the folder could not be created).
    Failed { error: Error },
}

const PROGRESS_EVERY: Duration = Duration::from_millis(200);

/// What the backup needs besides the selection.
pub struct Extras<'a> {
    /// The on-device helper dex, for the Resto items read through providers.
    pub helper_dex: Option<&'a Path>,
    /// Local scratch directory.
    pub tmp: &'a Path,
    /// Pauses the job while the phone is away or locked; without it, losing the phone stops
    /// the job.
    pub gate: Option<&'a dyn crate::pause::Gate>,
}

/// Errors that end the whole job rather than one item.
fn is_fatal(e: &Error) -> bool {
    matches!(e, Error::Cancelled | Error::DeviceUnavailable { .. } | Error::DiskFull)
}

/// Key of a Resto item in the job's per-item statuses.
pub fn other_key(item: crate::other::OtherItem) -> String {
    format!("other:{}", serde_json::to_value(item).unwrap().as_str().unwrap())
}

/// Runs the backup. Failures of single steps are recorded and the rest goes on (ui-spec §6).
/// While the phone is away or locked the job pauses and retries the step (see [`Extras::gate`]).
/// Cancelling, a full disk or losing the phone without a gate stops it; the manifest is then
/// written as incomplete and the backup can be resumed with [`BackupRequest::resume`].
/// The manifest is also rewritten after every item, so a crash leaves a resumable backup.
pub async fn run(
    device: &Device,
    info: &DeviceInfo,
    analysis: &crate::analysis::Analysis,
    req: &BackupRequest,
    extras: &Extras<'_>,
    cancel: &AtomicBool,
    mut on_event: impl FnMut(JobEvent),
) -> Result<JobResult> {
    // Every data step runs as root; without it the job would finish "successfully" with nothing copied.
    device.root().ok_or(Error::RootUnavailable)?;
    let apps = &analysis.apps;
    let accounts = &analysis.accounts;
    let start = Instant::now();
    let gate = extras.gate;

    let (dir, previous) = match &req.resume {
        Some(path) => {
            let dir = PathBuf::from(path);
            let m = Manifest::read(&dir.join(MANIFEST))?;
            if m.source.serial != info.serial {
                return Err(Error::WrongDevice { expected: format!("{} {}", m.source.manufacturer, m.source.model) });
            }
            (dir, Some(m))
        }
        None => (unique_dir(&Path::new(&req.dest_dir).join(crate::format::validate_name(&req.name)?), req.pack)?, None),
    };
    let name = dir.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let tag = format!("backup:{name}");
    let device = &device.tagged(&tag);
    let first_log_id = device.adb().log().next_id();

    let steps = plan(apps, req);
    let by_pkg: HashMap<&str, &AppInfo> = apps.iter().map(|a| (a.package.as_str(), a)).collect();
    let selections: HashMap<&str, &AppSelection> = req.apps.iter().map(|s| (s.package.as_str(), s)).collect();
    let packages: Vec<String> = req.apps.iter().filter(|s| by_pkg.contains_key(s.package.as_str())).map(|s| s.package.clone()).collect();
    let other_items = if analysis.other.is_some() { req.other.items() } else { vec![] };
    let other_estimate = |i: crate::other::OtherItem| {
        analysis.other.as_ref().map(|o| crate::other::backup::estimate(i, &req.other, o)).unwrap_or(0)
    };

    // What an earlier, interrupted run already saved: apps and items without errors.
    let mut manifest_apps: Vec<ManifestApp> = Vec::new();
    let mut other_entries = Vec::new();
    let mut files: Vec<FileEntry> = Vec::new();
    if let Some(prev) = &previous {
        let failed: Vec<&str> = prev.errors.iter().map(|e| e.package.as_deref().unwrap_or(&e.step)).collect();
        manifest_apps = prev.apps.iter().filter(|a| packages.contains(&a.package) && !failed.contains(&a.package.as_str())).cloned().collect();
        other_entries = prev
            .other
            .iter()
            .filter(|e| other_items.contains(&e.item) && !failed.contains(&other_key(e.item).as_str()))
            .cloned()
            .collect();
        files = prev
            .files
            .iter()
            .filter(|f| {
                manifest_apps.iter().any(|a| f.path.starts_with(&format!("apps/{}/", a.package)))
                    || other_entries.iter().any(|e| e.files.contains(&f.path))
            })
            .cloned()
            .collect();
    }
    let app_done = |pkg: &str, done: &[ManifestApp]| done.iter().any(|a| a.package == pkg);
    let done_estimate: u64 = steps.iter().filter(|s| app_done(&s.package, &manifest_apps)).map(|s| s.estimate).sum::<u64>()
        + other_entries.iter().map(|e| other_estimate(e.item)).sum::<u64>();
    let total_bytes: u64 = steps.iter().map(|s| s.estimate).sum::<u64>() + other_items.iter().map(|i| other_estimate(*i)).sum::<u64>();

    // Data is compressed, so the estimate is an upper bound of what is written.
    crate::library::check_space(total_bytes - done_estimate, &dir)?;
    std::fs::create_dir_all(&dir)?;
    let mut saved = req.clone();
    saved.password = None;
    saved.resume = None;
    write_json(&dir.join(REQUEST), &saved)?;

    let mut keys = packages.clone();
    keys.extend(other_items.iter().map(|i| other_key(*i)));
    on_event(JobEvent::Started { total_bytes, packages: keys });
    for a in &manifest_apps {
        on_event(JobEvent::Item { package: a.package.clone(), status: ItemStatus::Done });
    }
    for e in &other_entries {
        on_event(JobEvent::Item { package: other_key(e.item), status: ItemStatus::Done });
    }

    let created_at = previous
        .as_ref()
        .map(|m| m.created_at.clone())
        .unwrap_or_else(|| humantime::format_rfc3339_seconds(SystemTime::now()).to_string());
    let checkpoint = |apps: &[ManifestApp], other: &[crate::other::backup::OtherEntry], files: &[FileEntry], errors: &[ItemError], complete: bool| {
        Manifest {
            format_version: FORMAT_VERSION,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            created_at: created_at.clone(),
            name: name.clone(),
            source: SourceDevice {
                serial: info.serial.clone(),
                manufacturer: info.manufacturer.clone(),
                model: info.model.clone(),
                android_release: info.android_release.clone(),
                sdk: info.sdk,
                abis: info.abis.clone(),
                build_type: info.build_type.clone(),
                build_id: info.build_id.clone(),
                root_method: device.root(),
            },
            keystore_packages: apps
                .iter()
                .filter(|a| by_pkg.get(a.package.as_str()).is_some_and(|i| i.keystore))
                .map(|a| a.package.clone())
                .collect(),
            apps: apps.to_vec(),
            other: other.to_vec(),
            accounts: accounts.to_vec(),
            files: files.to_vec(),
            errors: errors.to_vec(),
            complete,
        }
    };
    checkpoint(&manifest_apps, &other_entries, &files, &[], false).write(&dir)?;

    let mut errors: Vec<ItemError> = Vec::new();
    let mut bytes_done = done_estimate;
    let mut stopped: Option<Error> = None;
    let mut last_emit = Instant::now() - PROGRESS_EVERY;
    let mut last_bytes = 0u64;

    'apps: for pkg in &packages {
        if app_done(pkg, &manifest_apps) {
            continue;
        }
        let app = by_pkg[pkg.as_str()];
        let sel = selections[pkg.as_str()];
        on_event(JobEvent::Item { package: pkg.clone(), status: ItemStatus::Running });
        // Leftovers of an interrupted run are redone from scratch.
        if previous.is_some() {
            let _ = std::fs::remove_dir_all(dir.join("apps").join(pkg));
        }
        let errors_before = errors.len();
        let files_before = files.len();
        let mut entry = ManifestApp {
            package: pkg.clone(),
            label: app.label.clone(),
            kind: app.kind,
            version_code: app.version_code,
            version_name: app.version_name.clone(),
            meta: rel(&["apps", pkg, "meta.json"]),
            apks: vec![],
            data: vec![],
            permissions: sel.permissions,
        };
        for step in steps.iter().filter(|s| &s.package == pkg) {
            let base = bytes_done;
            let mut attempt = 0;
            let result: Result<()> = loop {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    break Err(Error::Cancelled);
                }
                if let Err(e) = crate::pause::wait_ready(gate, device.serial(), cancel, &mut on_event).await {
                    break Err(e);
                }
                let r: Result<()> = match &step.kind {
                    StepKind::ForceStop => device.shell(step.command.as_deref().unwrap()).await.and_then(|o| o.ok().map(|_| ())),
                    StepKind::Meta => {
                        let mut meta = app.clone();
                        meta.permissions = if sel.permissions {
                            filter_permissions(&app.permissions, &sel.excluded_permissions)
                        } else {
                            Default::default()
                        };
                        let dest = step.dest.as_deref().unwrap();
                        write_json(&dir.join(dest), &meta).and_then(|_| add_file(&mut files, &dir, dest))
                    }
                    StepKind::Apk { .. } | StepKind::Data { .. } => {
                        let dest = step.dest.as_deref().unwrap();
                        let path = dir.join(dest);
                        std::fs::create_dir_all(path.parent().unwrap())?;
                        let level = matches!(step.kind, StepKind::Data { .. }).then_some(req.compression_level);
                        let label = step.label();
                        let r = root_to_file(device, step.command.as_deref().unwrap(), &path, level, cancel, |n| {
                            if last_emit.elapsed() >= PROGRESS_EVERY {
                                let done = base + n;
                                let speed = ((done.saturating_sub(last_bytes)) as f64 / last_emit.elapsed().as_secs_f64()) as u64;
                                on_event(JobEvent::Progress {
                                    bytes_done: done,
                                    total_bytes,
                                    package: pkg.clone(),
                                    step: label.clone(),
                                    speed,
                                });
                                last_emit = Instant::now();
                                last_bytes = done;
                            }
                        })
                        .await;
                        r.map(|s| {
                            bytes_done += s.bytes_in;
                            files.push(FileEntry { path: dest.to_string(), size: s.bytes_out, sha256: s.sha256 });
                            match &step.kind {
                                StepKind::Apk { .. } => entry.apks.push(dest.to_string()),
                                StepKind::Data { part } => entry.data.push(DataArchive { part: *part, path: dest.to_string() }),
                                _ => {}
                            }
                        })
                    }
                };
                match r {
                    Err(e) if crate::pause::should_retry(gate, device, &e, attempt).await => attempt += 1,
                    r => break r,
                }
            };
            // Keep the estimate moving even when the real size differed or the step failed.
            bytes_done = bytes_done.max(base + step.estimate);
            if let Err(error) = result {
                errors.push(ItemError { package: Some(pkg.clone()), step: step.label(), error: error.clone() });
                if is_fatal(&error) {
                    stopped = Some(error);
                    break;
                }
            }
        }
        let status = if errors.len() > errors_before { ItemStatus::Error } else { ItemStatus::Done };
        on_event(JobEvent::Item { package: pkg.clone(), status });
        if stopped.is_some() {
            // Not in the manifest, so resuming redoes it; its files are left out too.
            files.truncate(files_before);
            break 'apps;
        }
        manifest_apps.push(entry);
        checkpoint(&manifest_apps, &other_entries, &files, &errors, false).write(&dir)?;
    }

    // Resto tab items, with the helper deployed once for all of them if needed.
    let pending: Vec<_> = other_items.iter().copied().filter(|i| !other_entries.iter().any(|e| e.item == *i)).collect();
    if stopped.is_none() && !pending.is_empty() && let Some(oa) = &analysis.other {
        let needs_helper = pending.iter().any(|i| i.needs_helper());
        let helper = match (needs_helper, extras.helper_dex) {
            (true, Some(dex)) => crate::helper::Helper::deploy(device, dex, extras.tmp).await.ok(),
            _ => None,
        };
        let job = crate::other::backup::OtherJob {
            device,
            sdk: info.sdk,
            dir: &dir,
            tmp: extras.tmp,
            level: req.compression_level,
            cancel,
            analysis: oa,
            helper: helper.as_ref(),
        };
        for item in &pending {
            let key = other_key(*item);
            on_event(JobEvent::Item { package: key.clone(), status: ItemStatus::Running });
            on_event(JobEvent::Progress { bytes_done, total_bytes, package: key.clone(), step: key.clone(), speed: 0 });
            let files_before = files.len();
            let mut attempt = 0;
            // Reading is harmless, so an item cut by a disconnection is read again.
            let result = loop {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    break Err(Error::Cancelled);
                }
                if let Err(e) = crate::pause::wait_ready(gate, device.serial(), cancel, &mut on_event).await {
                    break Err(e);
                }
                match job.run(*item, &req.other, &mut files).await {
                    Err(e) if crate::pause::should_retry(gate, device, &e, attempt).await => {
                        files.truncate(files_before);
                        attempt += 1;
                    }
                    r => break r,
                }
            };
            let status = match result {
                Ok(entry) => {
                    other_entries.push(entry);
                    ItemStatus::Done
                }
                Err(error) => {
                    files.truncate(files_before);
                    errors.push(crate::other::backup::failure(*item, error.clone()));
                    if is_fatal(&error) {
                        stopped = Some(error);
                    }
                    ItemStatus::Error
                }
            };
            bytes_done += other_estimate(*item);
            on_event(JobEvent::Item { package: key, status });
            if stopped.is_some() {
                break;
            }
            checkpoint(&manifest_apps, &other_entries, &files, &errors, false).write(&dir)?;
        }
        if let Some(h) = helper {
            let _ = h.remove().await;
        }
    }

    // Session log (ui-spec §5 "Registro"), appended to the one of an earlier run, then the
    // manifest, which marks the backup as readable.
    let log_path = rel(&["report", "session.log"]);
    let mut log_text: Vec<String> = Vec::new();
    if previous.is_some()
        && let Ok(old) = std::fs::read_to_string(dir.join(&log_path))
    {
        log_text.push(old);
        log_text.push(format!("# resumed {}", humantime::format_rfc3339_seconds(SystemTime::now())));
    }
    log_text.extend(
        device
            .adb()
            .log()
            .entries()
            .iter()
            .filter(|e| e.id >= first_log_id && e.tag.as_deref() == Some(tag.as_str()))
            .map(log_entry_text),
    );
    std::fs::create_dir_all(dir.join("report"))?;
    std::fs::write(dir.join(&log_path), log_text.join("\n\n"))?;
    add_file(&mut files, &dir, &log_path)?;
    add_file(&mut files, &dir, REQUEST)?;

    let manifest = checkpoint(&manifest_apps, &other_entries, &files, &errors, stopped.is_none());
    manifest.write(&dir)?;

    let size_on_disk = manifest.files.iter().map(|f| f.size).sum();
    let mut path = dir.clone();
    if req.pack && stopped.is_none() {
        // The pack is about as big as the folder, which is removed only afterwards. Without
        // room the folder is kept and the problem reported.
        let packed = crate::format::packed_path(&dir);
        match crate::library::check_space(size_on_disk, &dir) {
            Ok(()) => {
                on_event(JobEvent::Packing);
                // The folder goes only after the pack has been read back correctly.
                match crate::library::pack_verified(&dir, &packed, req.password.as_deref()) {
                    Ok(()) => {
                        std::fs::remove_dir_all(&dir)?;
                        path = packed;
                    }
                    Err(error) => errors.push(ItemError { package: None, step: "pack".into(), error }),
                }
            }
            Err(error) => errors.push(ItemError { package: None, step: "pack".into(), error }),
        }
    }
    let result = JobResult {
        path: path.display().to_string(),
        cancelled: matches!(stopped, Some(Error::Cancelled)),
        errors,
        bytes: bytes_done,
        size_on_disk,
        duration_ms: start.elapsed().as_millis() as u32,
        restored: vec![],
        other: vec![],
    };
    on_event(JobEvent::Finished { result: result.clone() });
    Ok(result)
}

fn add_file(files: &mut Vec<FileEntry>, dir: &Path, rel_path: &str) -> Result<()> {
    let path = dir.join(rel_path);
    files.push(FileEntry {
        path: rel_path.to_string(),
        size: std::fs::metadata(&path)?.len(),
        sha256: sha256_file(&path)?,
    });
    Ok(())
}

/// `base`, or `base-2`, `base-3`… if it (or its packed file) already exists.
fn unique_dir(base: &Path, packed: bool) -> Result<PathBuf> {
    let taken = |p: &Path| p.exists() || (packed && crate::format::packed_path(p).exists());
    if !taken(base) {
        return Ok(base.to_path_buf());
    }
    for i in 2.. {
        let candidate = base.with_file_name(format!("{}-{i}", base.file_name().unwrap_or_default().to_string_lossy()));
        if !taken(&candidate) {
            return Ok(candidate);
        }
    }
    unreachable!()
}

pub fn log_entry_text(e: &LogEntry) -> String {
    let mut s = format!(
        "$ {} {}\n# exit {} · {} ms",
        e.program,
        e.args.iter().map(|a| shell_quote(a)).collect::<Vec<_>>().join(" "),
        e.exit_code.map(|c| c.to_string()).unwrap_or_else(|| "?".into()),
        e.duration_ms
    );
    if let Some(b) = e.streamed_bytes {
        s.push_str(&format!(" · {b} bytes streamed"));
    }
    if !e.stdout.is_empty() {
        s.push('\n');
        s.push_str(e.stdout.trim_end());
    }
    if !e.stderr.is_empty() {
        s.push_str("\n[stderr]\n");
        s.push_str(e.stderr.trim_end());
    }
    s
}

/// Whether `dir` looks like a finished backup folder.
pub fn is_backup_dir(dir: &Path) -> bool {
    dir.join(MANIFEST).is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::{AppKind, ApkFile, DataSizes, Permissions};

    fn app() -> AppInfo {
        AppInfo {
            package: "com.termux".into(),
            kind: AppKind::User,
            uid: 10328,
            version_code: 140,
            version_name: Some("0.140".into()),
            min_sdk: 24,
            target_sdk: 28,
            code_path: "/data/app/~~x==/com.termux-y==".into(),
            apks: vec![ApkFile { name: "base.apk".into(), size: 100 }, ApkFile { name: "split_config.en.apk".into(), size: 10 }],
            splits: vec!["base".into(), "config.en".into()],
            primary_abi: None,
            installer: None,
            signatures: vec![],
            shared_user: None,
            flags: vec![],
            data: DataSizes { ce: Some(1000), de: Some(20), ext_data: None, obb: None, media: None, ce_no_backup: 0, de_no_backup: 0 },
            permissions: Permissions::default(),
            keystore: false,
            exclusion: None,
            label: None,
            icon: None,
        }
    }

    fn request(data: Vec<DataPart>) -> BackupRequest {
        BackupRequest {
            dest_dir: "/tmp".into(),
            name: "x".into(),
            apps: vec![AppSelection {
                package: "com.termux".into(),
                apk: true,
                data,
                permissions: true,
                excluded_permissions: vec![],
            }],
            include_no_backup: false,
            compression_level: 3,
            pack: false,
            other: Default::default(),
            password: None,
            resume: None,
        }
    }

    #[test]
    fn plans_apks_force_stop_and_existing_data_parts() {
        let steps = plan(&[app()], &request(DataPart::ALL.to_vec()));
        let labels: Vec<String> = steps.iter().map(Step::label).collect();
        assert_eq!(labels, ["apk:base.apk", "apk:split_config.en.apk", "forceStop", "data:ce", "data:de", "meta"]);
        assert_eq!(
            steps[3].command.as_deref(),
            Some(
                "cd /data/data && find com.termux -type s | tar -cpf - -X /dev/stdin --exclude=com.termux/cache \
                 --exclude=com.termux/code_cache --exclude=com.termux/lib --exclude=com.termux/no_backup com.termux"
            )
        );
        assert_eq!(steps[3].dest.as_deref(), Some("apps/com.termux/data/ce.tar.zst"));
        assert_eq!(steps.iter().map(|s| s.estimate).sum::<u64>(), 1130);
    }

    #[test]
    fn estimate_adds_no_backup_only_when_included() {
        let mut a = app();
        a.data.ce_no_backup = 50;
        assert_eq!(size_of(&a, DataPart::Ce, false), Some(1000));
        assert_eq!(size_of(&a, DataPart::Ce, true), Some(1050));
        assert_eq!(size_of(&a, DataPart::De, true), Some(20));
    }

    #[test]
    fn filters_excluded_permissions() {
        let p = Permissions {
            runtime: vec!["android.permission.CAMERA".into(), "android.permission.RECORD_AUDIO".into()],
            app_ops: vec![crate::analysis::AppOp { op: "MANAGE_EXTERNAL_STORAGE".into(), mode: "allow".into() }],
            special: vec![SpecialAccess::BatteryUnrestricted],
            roles: vec!["android.app.role.BROWSER".into()],
        };
        let keys = permission_keys(&p);
        assert_eq!(keys.len(), 5);
        let excluded = vec![keys[0].clone(), keys[3].clone()];
        let f = filter_permissions(&p, &excluded);
        assert_eq!(f.runtime, ["android.permission.RECORD_AUDIO"]);
        assert!(f.special.is_empty());
        assert_eq!(f.app_ops.len(), 1);
        assert_eq!(f.roles.len(), 1);
    }

    #[test]
    fn no_force_stop_without_data() {
        let steps = plan(&[app()], &request(vec![]));
        assert!(steps.iter().all(|s| s.kind != StepKind::ForceStop));
    }

    /// A fake `adb`: streams "DATA" for every `shell -T` command and logs it. A `drop-<pkg>`
    /// file makes the next stream of that package fail as if the cable were pulled (and
    /// `get-state` then says so once).
    fn fake_adb(dir: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let script = dir.join("adb");
        std::fs::write(
            &script,
            format!(
                r#"#!/bin/sh
d={d}
case "$3" in
  get-state) if [ -e "$d/gone" ]; then rm "$d/gone"; echo "error: device not found" >&2; exit 1; fi; echo device ;;
  shell)
    if [ "$4" = "-T" ]; then
      echo "$5" >> "$d/streams"
      for p in com.a com.b; do
        case "$5" in *$p*) if [ -e "$d/drop-$p" ]; then rm "$d/drop-$p"; touch "$d/gone"; exit 1; fi ;; esac
      done
      printf DATA
    fi ;;
esac
exit 0
"#,
                d = dir.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        script
    }

    fn info() -> DeviceInfo {
        DeviceInfo {
            serial: "s".into(),
            manufacturer: "Google".into(),
            model: "Pixel".into(),
            android_release: "16".into(),
            sdk: 36,
            abis: vec![],
            build_type: "user".into(),
            build_id: String::new(),
            battery: None,
            storage: None,
            user_state: None,
            users: vec![],
            tools: vec![],
            adbd_root: true,
            adb_root_possible: true,
            su_path: None,
        }
    }

    fn two_apps() -> crate::analysis::Analysis {
        let mk = |p: &str| AppInfo { package: p.into(), code_path: format!("/data/app/{p}"), data: DataSizes { ce: Some(4), de: None, ext_data: None, obb: None, media: None, ce_no_backup: 0, de_no_backup: 0 }, ..app() };
        crate::analysis::Analysis {
            stage: crate::analysis::AnalysisStage::Done,
            apps: vec![mk("com.a"), mk("com.b")],
            accounts: vec![],
            other: None,
            errors: vec![],
        }
    }

    fn two_app_request(dest: &Path) -> BackupRequest {
        let sel = |p: &str| AppSelection { package: p.into(), apk: true, data: vec![DataPart::Ce], permissions: false, excluded_permissions: vec![] };
        BackupRequest { dest_dir: dest.display().to_string(), name: "B".into(), apps: vec![sel("com.a"), sel("com.b")], ..request(vec![]) }
    }

    fn streams(dir: &Path, pkg: &str) -> usize {
        std::fs::read_to_string(dir.join("streams")).unwrap_or_default().lines().filter(|l| l.contains(pkg)).count()
    }

    struct AlwaysReady;
    impl crate::pause::Gate for AlwaysReady {
        fn availability(&self, _: &str) -> crate::pause::Availability {
            crate::pause::Availability::Ready
        }
    }

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("adbkeep-backup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[tokio::test]
    async fn cancelled_backup_resumes_without_redoing_finished_apps() {
        let base = tmp("resume");
        let device = Device::new(crate::adb::Adb::new(fake_adb(&base), std::sync::Arc::new(crate::log::CommandLog::new())), "s")
            .with_root(Some(crate::root::RootMethod::AdbRoot));
        let analysis = two_apps();
        let req = two_app_request(&base);
        let extras = Extras { helper_dex: None, tmp: &base, gate: None };

        // Cancel as soon as the second app starts.
        let cancel = AtomicBool::new(false);
        let first = run(&device, &info(), &analysis, &req, &extras, &cancel, |e| {
            if matches!(&e, JobEvent::Item { package, status: ItemStatus::Running } if package == "com.b") {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        })
        .await
        .unwrap();
        assert!(first.cancelled);
        let dir = PathBuf::from(&first.path);
        let m = Manifest::read(&dir.join(MANIFEST)).unwrap();
        assert!(!m.complete);
        assert_eq!(m.apps.iter().map(|a| a.package.as_str()).collect::<Vec<_>>(), ["com.a"]);
        assert!(m.files.iter().all(|f| !f.path.contains("com.b")));
        assert!(crate::library::list(&base).unwrap()[0].resumable);
        let saved = saved_request(&dir).unwrap();
        assert_eq!(saved.apps.len(), 2);

        let a_streams = streams(&base, "com.a");
        let resume = BackupRequest { resume: Some(dir.display().to_string()), ..saved };
        let second = run(&device, &info(), &analysis, &resume, &extras, &AtomicBool::new(false), |_| {}).await.unwrap();
        assert_eq!(second.path, first.path);
        assert!(second.errors.is_empty(), "{:?}", second.errors);
        assert_eq!(streams(&base, "com.a"), a_streams, "com.a was saved again");
        let m2 = Manifest::read(&dir.join(MANIFEST)).unwrap();
        assert!(m2.complete);
        assert_eq!(m2.created_at, m.created_at);
        assert_eq!(m2.apps.len(), 2);
        // The fake streams "DATA", not a tar; everything else must match the manifest.
        let problems = crate::library::verify(&dir, |_, _| {}).unwrap().problems;
        assert!(problems.iter().all(|p| matches!(p.problem, crate::library::Problem::Corrupt { .. })), "{problems:?}");

        // Another phone cannot continue it.
        let other = DeviceInfo { serial: "x".into(), ..info() };
        let r = run(&device, &other, &analysis, &resume, &extras, &AtomicBool::new(false), |_| {}).await;
        assert!(matches!(r, Err(Error::WrongDevice { .. })));
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[tokio::test]
    async fn step_cut_by_a_disconnection_is_retried_with_a_gate() {
        let base = tmp("retry");
        let device = Device::new(crate::adb::Adb::new(fake_adb(&base), std::sync::Arc::new(crate::log::CommandLog::new())), "s")
            .with_root(Some(crate::root::RootMethod::AdbRoot));
        std::fs::write(base.join("drop-com.b"), b"").unwrap();
        let gate = AlwaysReady;
        let extras = Extras { helper_dex: None, tmp: &base, gate: Some(&gate) };
        let r = run(&device, &info(), &two_apps(), &two_app_request(&base), &extras, &AtomicBool::new(false), |_| {}).await.unwrap();
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        // base.apk, split, ce of com.b, plus the dropped attempt.
        assert_eq!(streams(&base, "com.b"), 4);

        // Without a gate the same cut is an error of that step.
        std::fs::write(base.join("drop-com.b"), b"").unwrap();
        let extras = Extras { helper_dex: None, tmp: &base, gate: None };
        let r = run(&device, &info(), &two_apps(), &two_app_request(&base), &extras, &AtomicBool::new(false), |_| {}).await.unwrap();
        assert_eq!(r.errors.len(), 1);
        std::fs::remove_dir_all(&base).unwrap();
    }
}
