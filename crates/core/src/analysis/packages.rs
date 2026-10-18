//! Parsing `dumpsys package packages` (backup-android.md points 1–3 and the system-apps section).

/// One `Package [name]` block of the `Packages:` section.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PackageDump {
    pub package: String,
    pub app_id: Option<u32>,
    pub code_path: String,
    pub version_code: u64,
    pub version_name: Option<String>,
    pub min_sdk: u32,
    pub target_sdk: u32,
    pub primary_abi: Option<String>,
    pub splits: Vec<String>,
    pub pkg_flags: Vec<String>,
    pub installer: Option<String>,
    /// Hashes from `signatures=PackageSignatures{… signatures:[…]}`.
    pub signatures: Vec<String>,
    /// e.g. `android.uid.system`.
    pub shared_user: Option<String>,
    /// Runtime permissions granted to the requested user.
    pub runtime_granted: Vec<String>,
}

impl PackageDump {
    pub fn is_system(&self) -> bool {
        self.pkg_flags.iter().any(|f| f == "SYSTEM")
    }

    pub fn is_updated_system(&self) -> bool {
        self.pkg_flags.iter().any(|f| f == "UPDATED_SYSTEM_APP")
    }
}

/// Parses the `Packages:` section; runtime permissions are taken from `User <user>:`.
pub fn parse_packages(out: &str, user: u32) -> Vec<PackageDump> {
    let mut result = Vec::new();
    let mut current: Option<PackageDump> = None;
    let mut in_packages = false;
    // Inside `User N:` of the wanted user, and inside its `runtime permissions:` list.
    let mut in_user = false;
    let mut in_runtime = false;
    let user_header = format!("User {user}:");

    for line in out.lines() {
        if !line.starts_with(' ') && !line.is_empty() {
            in_packages = line.trim_end() == "Packages:";
            if let Some(p) = current.take() {
                result.push(p);
            }
            continue;
        }
        if !in_packages {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let text = line.trim();
        if indent == 2 && let Some(rest) = text.strip_prefix("Package [") {
            if let Some(p) = current.take() {
                result.push(p);
            }
            let name = rest.split(']').next().unwrap_or_default();
            current = Some(PackageDump { package: name.to_string(), ..Default::default() });
            in_user = false;
            in_runtime = false;
            continue;
        }
        let Some(p) = current.as_mut() else { continue };
        if indent == 4 {
            in_user = text.starts_with(&user_header);
            in_runtime = false;
            parse_fields(p, text);
        } else if in_user && indent == 6 {
            in_runtime = text == "runtime permissions:";
        } else if in_user
            && in_runtime
            && indent == 8
            && let Some((perm, rest)) = text.split_once(": ")
            && rest.starts_with("granted=true")
        {
            p.runtime_granted.push(perm.to_string());
        }
    }
    if let Some(p) = current {
        result.push(p);
    }
    result
}

fn parse_fields(p: &mut PackageDump, text: &str) {
    if let Some(v) = text.strip_prefix("signatures=") {
        p.signatures = bracket_list(v.split("signatures:").nth(1).unwrap_or_default());
        return;
    }
    if let Some(v) = text.strip_prefix("pkgFlags=") {
        p.pkg_flags = v.trim_matches(|c| c == '[' || c == ']').split_whitespace().map(str::to_string).collect();
        return;
    }
    if let Some(v) = text.strip_prefix("splits=") {
        p.splits = bracket_list(v);
        return;
    }
    if let Some(v) = text.strip_prefix("sharedUser=") {
        // SharedUserSetting{4da409e android.uid.phone/1001}
        p.shared_user = v.split_whitespace().nth(1).and_then(|s| s.split('/').next()).map(str::to_string);
        return;
    }
    // Lines like `versionCode=11 minSdk=23 targetSdk=33`.
    for pair in text.split_whitespace() {
        let Some((k, v)) = pair.split_once('=') else { continue };
        match k {
            "appId" | "userId" => p.app_id = v.parse().ok(),
            "codePath" => p.code_path = v.to_string(),
            "versionCode" => p.version_code = v.parse().unwrap_or(0),
            "minSdk" => p.min_sdk = v.parse().unwrap_or(0),
            "targetSdk" => p.target_sdk = v.parse().unwrap_or(0),
            "primaryCpuAbi" => p.primary_abi = Some(v).filter(|v| *v != "null").map(str::to_string),
            "installerPackageName" => p.installer = Some(v).filter(|v| *v != "null").map(str::to_string),
            _ => {}
        }
    }
    if let Some(v) = text.strip_prefix("versionName=") {
        p.version_name = Some(v.to_string());
    }
}

/// `[a, b, c]` → `["a", "b", "c"]`.
fn bracket_list(v: &str) -> Vec<String> {
    let inner = v.split_once('[').map(|(_, r)| r).unwrap_or(v);
    let inner = inner.split(']').next().unwrap_or_default();
    inner.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()
}

/// `pm list packages -U` → (package, uid).
pub fn parse_pm_list_uids(out: &str) -> Vec<(String, u32)> {
    out.lines()
        .filter_map(|l| {
            let rest = l.trim().strip_prefix("package:")?;
            let (pkg, uid) = rest.split_once(" uid:")?;
            // Shared UIDs can be listed as "uid:1000,10123"; keep the first.
            Some((pkg.to_string(), uid.split(',').next()?.trim().parse().ok()?))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel() -> Vec<PackageDump> {
        let out = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/pixel10pro-a16/dumpsys-package-packages.txt"
        ))
        .unwrap();
        parse_packages(&out, 0)
    }

    #[test]
    fn stops_at_hidden_system_packages() {
        let pkgs = pixel();
        assert_eq!(pkgs.iter().filter(|p| p.package == "com.android.chrome").count(), 1);
        assert_eq!(pkgs.len(), 5);
    }

    #[test]
    fn parses_user_app() {
        let pkgs = pixel();
        let m = pkgs.iter().find(|p| p.package == "com.example.app").unwrap();
        assert_eq!(m.app_id, Some(10333));
        assert_eq!(m.version_code, 11);
        assert_eq!(m.version_name.as_deref(), Some("0.11"));
        assert_eq!((m.min_sdk, m.target_sdk), (23, 33));
        assert_eq!(m.splits, ["base"]);
        assert_eq!(m.signatures, ["588cb633"]);
        assert_eq!(m.installer, None);
        assert_eq!(m.primary_abi.as_deref(), Some("arm64-v8a"));
        assert!(m.code_path.starts_with("/data/app/"));
        assert!(!m.is_system());
        assert_eq!(
            m.runtime_granted,
            [
                "android.permission.READ_EXTERNAL_STORAGE",
                "android.permission.READ_PHONE_STATE",
                "android.permission.WRITE_EXTERNAL_STORAGE",
                "android.permission.RECORD_AUDIO"
            ]
        );
    }

    #[test]
    fn parses_splits_flags_and_shared_user() {
        let pkgs = pixel();
        let t = pkgs.iter().find(|p| p.package == "com.termux").unwrap();
        assert_eq!(t.splits, ["base", "config.arm64_v8a", "config.en", "config.es", "config.xxhdpi"]);
        assert_eq!(t.installer.as_deref(), Some("com.android.vending"));
        let chrome = pkgs.iter().find(|p| p.package == "com.android.chrome").unwrap();
        assert!(chrome.is_system() && chrome.is_updated_system());
        let phone = pkgs.iter().find(|p| p.package == "com.android.phone").unwrap();
        assert_eq!(phone.shared_user.as_deref(), Some("android.uid.phone"));
    }

    #[test]
    fn parses_pm_list_uids() {
        let out = "package:com.example.app uid:10333\npackage:com.termux uid:10328\npackage:android uid:1000,10001\n";
        assert_eq!(
            parse_pm_list_uids(out),
            [("com.example.app".into(), 10333), ("com.termux".into(), 10328), ("android".into(), 1000)]
        );
    }
}
