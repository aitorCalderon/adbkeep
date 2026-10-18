//! UIDs of apps with keys in Android Keystore (backup-android.md point 18).

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use crate::device::Device;
use crate::stream::root_to_file;
use crate::{Error, Result};

const KEYSTORE_DIR: &str = "/data/misc/keystore";

/// Android 12+ (Keystore2) copies `persistent.sqlite` (with its WAL) to `tmp` and queries it;
/// older versions list `user_0/<uid>_USRPKEY_<alias>` files.
pub async fn keystore_uids(device: &Device, sdk: u32, tmp: &Path) -> Result<BTreeSet<u32>> {
    if sdk >= 31 {
        keystore2_uids(device, tmp).await
    } else {
        let out = device.root_shell(&format!("ls {KEYSTORE_DIR}/user_0")).await?;
        Ok(legacy_uids(&out.stdout_str()))
    }
}

async fn keystore2_uids(device: &Device, tmp: &Path) -> Result<BTreeSet<u32>> {
    let listing = device.root_shell(&format!("ls {KEYSTORE_DIR}")).await?.ok()?.stdout_str();
    std::fs::create_dir_all(tmp)?;
    let never = AtomicBool::new(false);
    for name in ["persistent.sqlite", "persistent.sqlite-wal", "persistent.sqlite-shm"] {
        if listing.lines().any(|l| l.trim() == name) {
            root_to_file(device, &format!("cat {KEYSTORE_DIR}/{name}"), &tmp.join(name), None, &never, |_| {}).await?;
        }
    }
    let db = tmp.join("persistent.sqlite");
    let result = query_app_uids(&db);
    let _ = std::fs::remove_dir_all(tmp);
    result
}

/// `domain = 0` is APP; its `namespace` is the owning UID (domain 2 = SELinux namespaces).
fn query_app_uids(db: &Path) -> Result<BTreeSet<u32>> {
    let err = |e: rusqlite::Error| Error::Database { reason: e.to_string() };
    let conn = rusqlite::Connection::open(db).map_err(err)?;
    let mut stmt = conn
        .prepare("select distinct namespace from keyentry where domain = 0 and namespace >= 10000")
        .map_err(err)?;
    let uids = stmt
        .query_map([], |row| row.get::<_, i64>(0))
        .map_err(err)?
        .filter_map(|r| r.ok())
        .filter_map(|n| u32::try_from(n).ok())
        .collect();
    Ok(uids)
}

fn legacy_uids(listing: &str) -> BTreeSet<u32> {
    listing
        .lines()
        .filter_map(|l| l.trim().split_once('_'))
        .filter_map(|(uid, _)| uid.parse::<u32>().ok())
        .filter(|uid| *uid >= 10000)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_listing() {
        let ls = "10123_USRPKEY_alias\n10123_USRCERT_alias\n1000_USRPKEY_x\n10456_CACERT_y\n.masterkey\n";
        assert_eq!(legacy_uids(ls), BTreeSet::from([10123, 10456]));
    }

    #[test]
    fn keystore2_query() {
        let dir = std::env::temp_dir().join(format!("adbkeep-ks-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("persistent.sqlite");
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "create table keyentry (id integer, domain integer, namespace integer);
             insert into keyentry values (1, 0, 10328), (2, 0, 10328), (3, 2, 102), (4, 0, 1000), (5, 0, 10146);",
        )
        .unwrap();
        drop(conn);
        assert_eq!(query_app_uids(&db).unwrap(), BTreeSet::from([10146, 10328]));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
