//! Can each app of a backup be restored on the target phone? (ui-spec §4.1, backup-android.md
//! point 1 and the system-apps section). Reasons are codes; the UI words them.

use serde::Serialize;

use crate::analysis::installed::Installed;
use crate::analysis::{AppInfo, AppKind};
use crate::device::probe::DeviceInfo;
use crate::format::ManifestApp;

/// Android 14 refuses to install apps targeting SDK < 23.
const MIN_TARGET_SDK_34: u32 = 23;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum CompatStatus {
    Ok,
    Limited,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Reason {
    /// ⛔ `minSdk` above the target's SDK.
    #[serde(rename_all = "camelCase")]
    MinSdkTooHigh { min_sdk: u32, target_sdk: u32 },
    /// ⛔ Android 14+ blocks apps targeting SDK < 23 (unless forced, unverified).
    #[serde(rename_all = "camelCase")]
    TargetSdkTooLow { app_target_sdk: u32 },
    /// ⛔ Native code for a processor type the target lacks.
    #[serde(rename_all = "camelCase")]
    AbiMismatch { app_abi: String, device_abis: Vec<String> },
    /// ⛔ Installed on the target with another signature.
    SignatureMismatch,
    /// ⛔ The backup has no APK and the app is not installed on the target.
    NoApk,
    /// ⚠️ A system app being installed as a normal app: may not fully work.
    SystemAppAsUser,
    /// ⚠️ Data protected with keys tied to the source phone (point 18).
    Keystore,
    /// ⚠️ The target has a newer version; only data is restored onto it.
    #[serde(rename_all = "camelCase")]
    InstalledNewer { installed: u64, backup: u64 },
    /// ⚠️ It will become the default for a role another app holds now.
    #[serde(rename_all = "camelCase")]
    RoleTaken { role: String, holder: String },
}

impl Reason {
    fn blocks(&self) -> bool {
        matches!(
            self,
            Reason::MinSdkTooHigh { .. }
                | Reason::TargetSdkTooLow { .. }
                | Reason::AbiMismatch { .. }
                | Reason::SignatureMismatch
                | Reason::NoApk
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum PlannedAction {
    Install,
    Update,
    /// Installed with the same or a newer version: only data and permissions.
    DataOnly,
    Skip,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AppCompat {
    pub package: String,
    pub status: CompatStatus,
    pub reasons: Vec<Reason>,
    pub action: PlannedAction,
    /// Data can be restored: the backup has it and the app will be installed at ≥ its version.
    pub data_allowed: bool,
    pub installed_version: Option<u64>,
    pub installed_signatures: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CompatOptions {
    /// `--bypass-low-target-sdk-block` (not verified in practice).
    pub bypass_low_target_sdk: bool,
}

pub fn check(entry: &ManifestApp, meta: &AppInfo, target: &DeviceInfo, installed: &Installed, opts: CompatOptions) -> AppCompat {
    let mut reasons = Vec::new();
    let current = installed.packages.get(&entry.package);
    let has_apk = !entry.apks.is_empty();

    let action = match current {
        Some(cur) if !cur.signatures.is_empty() && !meta.signatures.is_empty() && cur.signatures != meta.signatures => {
            reasons.push(Reason::SignatureMismatch);
            PlannedAction::Skip
        }
        Some(cur) if cur.version_code >= entry.version_code => {
            if cur.version_code > entry.version_code {
                reasons.push(Reason::InstalledNewer { installed: cur.version_code, backup: entry.version_code });
            }
            PlannedAction::DataOnly
        }
        // Installed but older: update from the backup when possible, else keep it without data.
        Some(_) if !has_apk => PlannedAction::DataOnly,
        Some(_) => PlannedAction::Update,
        None if !has_apk => {
            reasons.push(Reason::NoApk);
            PlannedAction::Skip
        }
        None => PlannedAction::Install,
    };

    if matches!(action, PlannedAction::Install | PlannedAction::Update) {
        if meta.min_sdk > target.sdk {
            reasons.push(Reason::MinSdkTooHigh { min_sdk: meta.min_sdk, target_sdk: target.sdk });
        }
        if target.sdk >= 34 && meta.target_sdk < MIN_TARGET_SDK_34 && !opts.bypass_low_target_sdk {
            reasons.push(Reason::TargetSdkTooLow { app_target_sdk: meta.target_sdk });
        }
        if let Some(abi) = &meta.primary_abi
            && !target.abis.iter().any(|a| a == abi)
        {
            reasons.push(Reason::AbiMismatch { app_abi: abi.clone(), device_abis: target.abis.clone() });
        }
        if meta.kind == AppKind::UpdatedSystem && current.is_none() {
            reasons.push(Reason::SystemAppAsUser);
        }
    }
    if meta.keystore && !entry.data.is_empty() {
        reasons.push(Reason::Keystore);
    }
    if entry.permissions {
        for role in &meta.permissions.roles {
            if let Some(holder) = installed.roles.get(role).and_then(|h| h.iter().find(|h| *h != &entry.package)) {
                reasons.push(Reason::RoleTaken { role: role.clone(), holder: holder.clone() });
            }
        }
    }

    let blocked = reasons.iter().any(Reason::blocks);
    let action = if blocked { PlannedAction::Skip } else { action };
    // Data of a newer app version over an older install usually fails (point 1).
    let older_install = matches!(action, PlannedAction::DataOnly) && current.is_some_and(|c| c.version_code < entry.version_code);
    let data_allowed = !entry.data.is_empty() && action != PlannedAction::Skip && !older_install;
    let status = if blocked {
        CompatStatus::Blocked
    } else if reasons.is_empty() {
        CompatStatus::Ok
    } else {
        CompatStatus::Limited
    };
    AppCompat {
        package: entry.package.clone(),
        status,
        reasons,
        action,
        data_allowed,
        installed_version: current.map(|c| c.version_code),
        installed_signatures: current.map(|c| c.signatures.clone()).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::installed::InstalledPackage;
    use crate::analysis::{DataSizes, Permissions};
    use crate::format::{DataArchive, DataPart};

    fn meta() -> AppInfo {
        AppInfo {
            package: "com.x".into(),
            kind: AppKind::User,
            uid: 10100,
            version_code: 10,
            version_name: None,
            min_sdk: 26,
            target_sdk: 34,
            code_path: String::new(),
            apks: vec![],
            splits: vec![],
            primary_abi: Some("arm64-v8a".into()),
            installer: None,
            signatures: vec!["aa".into()],
            shared_user: None,
            flags: vec![],
            data: DataSizes::default(),
            permissions: Permissions::default(),
            keystore: false,
            exclusion: None,
            label: None,
            icon: None,
        }
    }

    fn entry(apk: bool, data: bool) -> ManifestApp {
        ManifestApp {
            package: "com.x".into(),
            label: None,
            kind: AppKind::User,
            version_code: 10,
            version_name: None,
            meta: String::new(),
            apks: if apk { vec!["apps/com.x/apk/base.apk".into()] } else { vec![] },
            data: if data { vec![DataArchive { part: DataPart::Ce, path: String::new() }] } else { vec![] },
            permissions: true,
        }
    }

    fn target(sdk: u32) -> DeviceInfo {
        DeviceInfo {
            serial: "t".into(),
            manufacturer: String::new(),
            model: String::new(),
            android_release: String::new(),
            sdk,
            abis: vec!["arm64-v8a".into(), "armeabi-v7a".into()],
            build_type: "user".into(),
            build_id: String::new(),
            battery: None,
            storage: None,
            user_state: None,
            users: vec![],
            tools: vec![],
            adbd_root: false,
            adb_root_possible: false,
            su_path: None,
        }
    }

    fn with(pkg_version: u64, sig: &str) -> Installed {
        let mut i = Installed::default();
        i.packages.insert(
            "com.x".into(),
            InstalledPackage { uid: Some(10200), version_code: pkg_version, signatures: vec![sig.into()], system: false },
        );
        i
    }

    #[test]
    fn fresh_install_is_ok() {
        let c = check(&entry(true, true), &meta(), &target(36), &Installed::default(), CompatOptions::default());
        assert_eq!((c.status, c.action, c.data_allowed), (CompatStatus::Ok, PlannedAction::Install, true));
    }

    #[test]
    fn blocking_reasons() {
        let mut m = meta();
        m.min_sdk = 35;
        let c = check(&entry(true, true), &m, &target(34), &Installed::default(), CompatOptions::default());
        assert_eq!(c.status, CompatStatus::Blocked);
        assert_eq!(c.action, PlannedAction::Skip);
        assert!(!c.data_allowed);
        assert_eq!(c.reasons, [Reason::MinSdkTooHigh { min_sdk: 35, target_sdk: 34 }]);

        let mut m = meta();
        m.target_sdk = 22;
        let c = check(&entry(true, false), &m, &target(34), &Installed::default(), CompatOptions::default());
        assert!(matches!(c.reasons[0], Reason::TargetSdkTooLow { .. }));
        let c = check(&entry(true, false), &m, &target(34), &Installed::default(), CompatOptions { bypass_low_target_sdk: true });
        assert_eq!(c.status, CompatStatus::Ok);

        let mut m = meta();
        m.primary_abi = Some("x86_64".into());
        let c = check(&entry(true, false), &m, &target(36), &Installed::default(), CompatOptions::default());
        assert!(matches!(c.reasons[0], Reason::AbiMismatch { .. }));

        let c = check(&entry(true, true), &meta(), &target(36), &with(10, "bb"), CompatOptions::default());
        assert_eq!(c.reasons, [Reason::SignatureMismatch]);

        let c = check(&entry(false, true), &meta(), &target(36), &Installed::default(), CompatOptions::default());
        assert_eq!(c.reasons, [Reason::NoApk]);
    }

    #[test]
    fn installed_versions() {
        let same = check(&entry(true, true), &meta(), &target(36), &with(10, "aa"), CompatOptions::default());
        assert_eq!((same.status, same.action, same.data_allowed), (CompatStatus::Ok, PlannedAction::DataOnly, true));

        let newer = check(&entry(true, true), &meta(), &target(36), &with(12, "aa"), CompatOptions::default());
        assert_eq!((newer.status, newer.action), (CompatStatus::Limited, PlannedAction::DataOnly));

        let older = check(&entry(true, true), &meta(), &target(36), &with(8, "aa"), CompatOptions::default());
        assert_eq!((older.action, older.data_allowed), (PlannedAction::Update, true));

        // Older install and no APK in the backup: keep it, but its data would not fit.
        let no_apk = check(&entry(false, true), &meta(), &target(36), &with(8, "aa"), CompatOptions::default());
        assert_eq!((no_apk.action, no_apk.data_allowed), (PlannedAction::DataOnly, false));
    }

    #[test]
    fn limited_reasons() {
        let mut m = meta();
        m.keystore = true;
        m.permissions.roles = vec!["android.app.role.BROWSER".into()];
        let mut installed = Installed::default();
        installed.roles.insert("android.app.role.BROWSER".into(), vec!["com.android.chrome".into()]);
        let c = check(&entry(true, true), &m, &target(36), &installed, CompatOptions::default());
        assert_eq!(c.status, CompatStatus::Limited);
        assert_eq!(
            c.reasons,
            [
                Reason::Keystore,
                Reason::RoleTaken { role: "android.app.role.BROWSER".into(), holder: "com.android.chrome".into() }
            ]
        );
    }
}
