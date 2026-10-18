//! "Mis copias" (ui-spec §5): listing, reading, verifying, deleting and (un)packing backups.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::archive;
use crate::format::{MANIFEST, Manifest, PACKED_EXT};
use crate::stream::hex;
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum BackupStatus {
    Complete,
    /// Cancelled or the phone went away; the manifest says what is there.
    Incomplete,
    /// No manifest: the app stopped in the middle (crash, closed). Only deletable.
    Interrupted,
    /// Packed with a password not given yet this session.
    Encrypted,
    Unreadable { error: Error },
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupListing {
    pub path: String,
    pub name: String,
    pub packed: bool,
    pub status: BackupStatus,
    /// Packed with a password.
    pub encrypted: bool,
    /// An incomplete backup folder that can be continued from the phone it was made from.
    pub resumable: bool,
    /// Bytes on disk.
    pub size: u64,
    pub manifest: Option<Manifest>,
}

/// Backups directly inside `dir`, newest first.
pub fn list(dir: &Path) -> Result<Vec<BackupListing>> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return Ok(out) };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_file() && path.extension().is_some_and(|e| e == PACKED_EXT) {
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            let encrypted = archive::is_encrypted(&path);
            let (status, manifest) = match archive::read_manifest(&path) {
                Ok(m) => (status_of(&m), Some(m)),
                Err(Error::PasswordRequired) => (BackupStatus::Encrypted, None),
                Err(error) => (BackupStatus::Unreadable { error }, None),
            };
            let name = name.trim_end_matches(&format!(".{PACKED_EXT}")).to_string();
            out.push(BackupListing {
                path: path.display().to_string(),
                name,
                packed: true,
                status,
                encrypted,
                resumable: false,
                size,
                manifest,
            });
        } else if ft.is_dir() {
            let manifest_path = path.join(MANIFEST);
            let (status, manifest) = if manifest_path.is_file() {
                match Manifest::read(&manifest_path) {
                    Ok(m) => (status_of(&m), Some(m)),
                    Err(error) => (BackupStatus::Unreadable { error }, None),
                }
            } else if path.join("apps").is_dir() || path.join("other").is_dir() {
                (BackupStatus::Interrupted, None)
            } else {
                continue; // Not ours.
            };
            let resumable = status == BackupStatus::Incomplete && path.join(crate::backup::REQUEST).is_file();
            out.push(BackupListing {
                path: path.display().to_string(),
                name,
                packed: false,
                status,
                encrypted: false,
                resumable,
                size: dir_size(&path),
                manifest,
            });
        }
    }
    out.sort_by(|a, b| {
        let key = |l: &BackupListing| l.manifest.as_ref().map(|m| m.created_at.clone()).unwrap_or_default();
        key(b).cmp(&key(a)).then(b.name.cmp(&a.name))
    });
    Ok(out)
}

fn status_of(m: &Manifest) -> BackupStatus {
    if m.complete { BackupStatus::Complete } else { BackupStatus::Incomplete }
}

fn dir_size(dir: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(e.path()),
                Ok(_) => total += e.metadata().map(|m| m.len()).unwrap_or(0),
                Err(_) => {}
            }
        }
    }
    total
}

pub fn is_packed(path: &Path) -> bool {
    path.is_file() && path.extension().is_some_and(|e| e == PACKED_EXT)
}

pub fn read_manifest(path: &Path) -> Result<Manifest> {
    if is_packed(path) { archive::read_manifest(path) } else { Manifest::read(&path.join(MANIFEST)) }
}

/// Reads one file of a backup (folder or packed), at most `limit` bytes.
pub fn read_file(path: &Path, rel: &str, limit: usize) -> Result<Vec<u8>> {
    with_file(path, rel, |r| {
        let mut buf = Vec::new();
        r.take(limit as u64).read_to_end(&mut buf)?;
        Ok(buf)
    })
}

/// Runs `f` on a reader of one file of the backup (folder or packed).
pub fn with_file<T>(path: &Path, rel: &str, f: impl FnOnce(&mut dyn Read) -> Result<T>) -> Result<T> {
    check_rel(rel)?;
    if is_packed(path) {
        let mut tar = archive::open(path)?;
        for entry in tar.entries()? {
            let mut entry = entry?;
            if entry.path()?.to_string_lossy() == rel {
                return f(&mut entry);
            }
        }
        Err(Error::Io { reason: format!("{rel}: not in the backup") })
    } else {
        f(&mut BufReader::new(File::open(path.join(rel))?))
    }
}

/// Several files of the backup in one pass (a packed backup is read once); files that are
/// not there are left out. Each is cut at `limit` bytes.
pub fn read_files(path: &Path, rels: &[&str], limit: usize) -> Result<HashMap<String, Vec<u8>>> {
    for rel in rels {
        check_rel(rel)?;
    }
    let mut out = HashMap::new();
    if is_packed(path) {
        let mut tar = archive::open(path)?;
        for entry in tar.entries()? {
            let entry = entry?;
            let name = entry.path()?.to_string_lossy().into_owned();
            if rels.contains(&name.as_str()) {
                let mut buf = Vec::new();
                entry.take(limit as u64).read_to_end(&mut buf)?;
                out.insert(name, buf);
                if out.len() == rels.len() {
                    break;
                }
            }
        }
    } else {
        for rel in rels {
            if let Ok(file) = File::open(path.join(rel)) {
                let mut buf = Vec::new();
                file.take(limit as u64).read_to_end(&mut buf)?;
                out.insert(rel.to_string(), buf);
            }
        }
    }
    Ok(out)
}

/// A file of the backup as text for the viewer: ABX (binary XML) is converted to XML.
pub fn read_text(path: &Path, rel: &str, limit: usize) -> Result<String> {
    let bytes = read_file(path, rel, limit)?;
    if crate::abx::is_abx(&bytes)
        && let Ok(elements) = crate::abx::parse_all(&bytes)
    {
        return Ok(crate::abx::to_xml(&elements));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Copies one file of the backup to `dest`.
pub fn export_file(path: &Path, rel: &str, dest: &Path) -> Result<u64> {
    with_file(path, rel, |r| {
        let partial = crate::format::with_suffix(dest, "partial");
        let result = std::io::copy(r, &mut std::io::BufWriter::new(File::create(&partial)?));
        match result {
            Ok(n) => {
                std::fs::rename(&partial, dest)?;
                Ok(n)
            }
            Err(e) => {
                let _ = std::fs::remove_file(&partial);
                Err(e.into())
            }
        }
    })
}

/// Rejects absolute paths and `..` so reads stay inside the backup.
fn check_rel(rel: &str) -> Result<()> {
    let p = Path::new(rel);
    if p.is_absolute() || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(Error::Io { reason: format!("{rel}: invalid path") });
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Problem {
    Missing,
    SizeMismatch { expected: u64, actual: u64 },
    HashMismatch,
    /// The compressed tar cannot be read to the end.
    Corrupt { reason: String },
    /// In the backup but not in the manifest.
    Unexpected,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FileProblem {
    pub path: String,
    pub problem: Problem,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VerifyReport {
    pub files_checked: u32,
    pub problems: Vec<FileProblem>,
}

/// Checks every file of the manifest: present, same size and SHA-256, and each `.tar.zst`
/// decompresses and lists to the end (ui-spec §3.7 / §5 "Verificar integridad").
/// `on_progress(done, total)` counts files.
pub fn verify(path: &Path, mut on_progress: impl FnMut(u32, u32)) -> Result<VerifyReport> {
    let manifest = read_manifest(path)?;
    let expected: HashMap<&str, (u64, &str)> =
        manifest.files.iter().map(|f| (f.path.as_str(), (f.size, f.sha256.as_str()))).collect();
    let total = expected.len() as u32;
    let mut problems = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut check = |rel: &str, reader: &mut dyn Read, problems: &mut Vec<FileProblem>| -> Result<()> {
        let Some(&(size, sha)) = expected.get(rel) else {
            if rel != MANIFEST {
                problems.push(FileProblem { path: rel.to_string(), problem: Problem::Unexpected });
            }
            return Ok(());
        };
        seen.push(rel.to_string());
        let (actual_size, actual_sha, corrupt) = hash_and_check(rel, reader)?;
        if actual_size != size {
            problems.push(FileProblem { path: rel.into(), problem: Problem::SizeMismatch { expected: size, actual: actual_size } });
        } else if actual_sha != sha {
            problems.push(FileProblem { path: rel.into(), problem: Problem::HashMismatch });
        } else if let Some(reason) = corrupt {
            problems.push(FileProblem { path: rel.into(), problem: Problem::Corrupt { reason } });
        }
        on_progress(seen.len() as u32, total);
        Ok(())
    };

    if is_packed(path) {
        let mut tar = archive::open(path)?;
        for entry in tar.entries()? {
            let mut entry = entry?;
            let rel = entry.path()?.to_string_lossy().replace('\\', "/");
            check(&rel, &mut entry, &mut problems)?;
        }
    } else {
        for f in &manifest.files {
            // Files that cannot be opened are reported as missing below.
            if let Ok(file) = File::open(path.join(&f.path)) {
                check(&f.path, &mut BufReader::new(file), &mut problems)?;
            }
        }
    }
    for f in &manifest.files {
        if !seen.contains(&f.path) {
            problems.push(FileProblem { path: f.path.clone(), problem: Problem::Missing });
        }
    }
    Ok(VerifyReport { files_checked: total, problems })
}

/// Size and SHA-256 of everything read from `reader`; for `.tar.zst`, also whether the tar
/// inside reads to the end (in the same pass).
fn hash_and_check(rel: &str, reader: &mut dyn Read) -> Result<(u64, String, Option<String>)> {
    let state = Rc::new(RefCell::new((Sha256::new(), 0u64)));
    let mut hashing = HashReader { inner: reader, state: state.clone() };
    let mut corrupt = None;
    if rel.ends_with(".tar.zst") {
        let result = (|| -> std::io::Result<()> {
            let dec = zstd::Decoder::new(&mut hashing)?;
            let mut tar = tar::Archive::new(dec);
            for entry in tar.entries()? {
                std::io::copy(&mut entry?, &mut std::io::sink())?;
            }
            Ok(())
        })();
        if let Err(e) = result {
            corrupt = Some(e.to_string());
        }
    }
    // Hash whatever the decoder did not consume (or the whole file for other types).
    std::io::copy(&mut hashing, &mut std::io::sink())?;
    drop(hashing);
    let (hasher, size) = Rc::try_unwrap(state).map_err(|_| Error::Io { reason: "hash state".into() })?.into_inner();
    Ok((size, hex(&hasher.finalize()), corrupt))
}

struct HashReader<'a> {
    inner: &'a mut dyn Read,
    state: Rc<RefCell<(Sha256, u64)>>,
}

impl Read for HashReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        let mut s = self.state.borrow_mut();
        s.0.update(&buf[..n]);
        s.1 += n as u64;
        Ok(n)
    }
}

/// Deletes a backup folder or packed file. Refuses anything that does not look like a backup.
pub fn delete(path: &Path) -> Result<()> {
    if is_packed(path) {
        archive::forget(path);
        std::fs::remove_file(path)?;
    } else if path.is_dir() && (path.join(MANIFEST).is_file() || path.join("apps").is_dir() || path.join("other").is_dir()) {
        std::fs::remove_dir_all(path)?;
    } else {
        return Err(Error::Io { reason: format!("{}: not a backup", path.display()) });
    }
    Ok(())
}

/// Packs `dir` into `dest` and reads the result back (sizes, hashes, readable tars). On any
/// problem `dest` is removed and `dir` is left alone, so the caller can delete `dir` safely
/// after an `Ok`.
pub fn pack_verified(dir: &Path, dest: &Path, password: Option<&str>) -> Result<()> {
    archive::pack(dir, dest, password)?;
    let problem = match verify(dest, |_, _| {}) {
        Ok(report) if report.problems.is_empty() => return Ok(()),
        Ok(report) => format!("{} problem(s), first: {}", report.problems.len(), report.problems[0].path),
        Err(e) => e.to_string(),
    };
    archive::forget(dest);
    let _ = std::fs::remove_file(dest);
    Err(Error::Io { reason: format!("{}: pack failed verification ({problem})", dest.display()) })
}

/// Folder → `<folder>.adbkeep` (encrypted when a password is given), removing the folder.
/// Returns the new path.
pub fn pack(dir: &Path, password: Option<&str>) -> Result<PathBuf> {
    if !dir.join(MANIFEST).is_file() {
        return Err(Error::Io { reason: format!("{}: not a finished backup", dir.display()) });
    }
    let dest = crate::format::packed_path(dir);
    if dest.exists() {
        return Err(Error::Io { reason: format!("{} already exists", dest.display()) });
    }
    check_space(dir_size(dir), dir)?;
    pack_verified(dir, &dest, password)?;
    std::fs::remove_dir_all(dir)?;
    Ok(dest)
}

/// `<name>.adbkeep` → folder `<name>`, removing the file. Returns the new path.
pub fn unpack(file: &Path) -> Result<PathBuf> {
    let dest = file.with_extension("");
    if dest.exists() {
        return Err(Error::Io { reason: format!("{} already exists", dest.display()) });
    }
    let manifest = read_manifest(file)?;
    check_space(manifest.files.iter().map(|f| f.size).sum(), file)?;
    let partial = file.with_extension("unpacking");
    if let Err(e) = archive::unpack(file, &partial) {
        let _ = std::fs::remove_dir_all(&partial);
        return Err(e);
    }
    std::fs::rename(&partial, &dest)?;
    archive::forget(file);
    std::fs::remove_file(file)?;
    Ok(dest)
}

/// Fails with [`Error::NotEnoughSpace`] unless `needed` bytes fit on the disk holding `near`
/// (a file or folder there, or one to be created).
pub fn check_space(needed: u64, near: &Path) -> Result<()> {
    let mut dir = near;
    while !dir.is_dir() {
        dir = dir.parent().ok_or_else(|| Error::Io { reason: format!("{}: no such folder", near.display()) })?;
    }
    let available = fs4::available_space(dir)?;
    if needed > available {
        return Err(Error::NotEnoughSpace { needed, available });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{FORMAT_VERSION, FileEntry, SourceDevice};
    use crate::stream::sha256_file;
    use std::io::Write;

    fn make_tar_zst(path: &Path) {
        let mut tar = tar::Builder::new(zstd::Encoder::new(File::create(path).unwrap(), 3).unwrap());
        let data = b"hello";
        let mut h = tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(0o644);
        h.set_cksum();
        tar.append_data(&mut h, "com.x/files/a.txt", &data[..]).unwrap();
        tar.into_inner().unwrap().finish().unwrap();
    }

    fn make_backup(base: &Path, name: &str) -> PathBuf {
        let dir = base.join(name);
        std::fs::create_dir_all(dir.join("apps/com.x/data")).unwrap();
        let tz = dir.join("apps/com.x/data/ce.tar.zst");
        make_tar_zst(&tz);
        std::fs::write(dir.join("apps/com.x/meta.json"), b"{}").unwrap();
        let files = ["apps/com.x/data/ce.tar.zst", "apps/com.x/meta.json"]
            .iter()
            .map(|r| FileEntry {
                path: r.to_string(),
                size: std::fs::metadata(dir.join(r)).unwrap().len(),
                sha256: sha256_file(&dir.join(r)).unwrap(),
            })
            .collect();
        Manifest {
            format_version: FORMAT_VERSION,
            app_version: "0".into(),
            created_at: "2026-10-14T12:00:00Z".into(),
            name: name.into(),
            source: SourceDevice {
                serial: "s".into(),
                manufacturer: "Google".into(),
                model: "Pixel".into(),
                android_release: "16".into(),
                sdk: 36,
                abis: vec![],
                build_type: "user".into(),
                build_id: String::new(),
                root_method: None,
            },
            apps: vec![],
            other: vec![],
            accounts: vec![],
            keystore_packages: vec![],
            files,
            errors: vec![],
            complete: true,
        }
        .write(&dir)
        .unwrap();
        dir
    }

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("adbkeep-lib-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn lists_folders_packed_and_interrupted() {
        let base = tmp("list");
        let dir = make_backup(&base, "A");
        make_backup(&base, "B");
        pack(&base.join("B"), None).unwrap();
        std::fs::create_dir_all(base.join("C/apps/com.y/apk")).unwrap();
        std::fs::create_dir_all(base.join("unrelated")).unwrap();
        let list = list(&base).unwrap();
        let names: Vec<(&str, bool, &BackupStatus)> = list.iter().map(|l| (l.name.as_str(), l.packed, &l.status)).collect();
        assert_eq!(names.len(), 3);
        assert!(names.contains(&("A", false, &BackupStatus::Complete)));
        assert!(names.contains(&("B", true, &BackupStatus::Complete)));
        assert!(names.contains(&("C", false, &BackupStatus::Interrupted)));
        assert_eq!(read_file(&dir, "apps/com.x/meta.json", 100).unwrap(), b"{}");
        assert_eq!(read_file(&base.join("B.adbkeep"), "apps/com.x/meta.json", 100).unwrap(), b"{}");
        assert!(read_file(&dir, "../x", 10).is_err());
        let many = read_files(&base.join("B.adbkeep"), &["apps/com.x/meta.json", "nope"], 100).unwrap();
        assert_eq!(many.len(), 1);
        assert_eq!(many["apps/com.x/meta.json"], b"{}");
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn verify_detects_problems_in_folder_and_packed() {
        let base = tmp("verify");
        let dir = make_backup(&base, "A");
        assert_eq!(verify(&dir, |_, _| {}).unwrap().problems, vec![]);

        // Same-size corruption of the compressed tar, plus a missing file.
        let tz = dir.join("apps/com.x/data/ce.tar.zst");
        let mut bytes = std::fs::read(&tz).unwrap();
        let mid = bytes.len() / 2;
        bytes[mid] ^= 0xff;
        std::fs::write(&tz, &bytes).unwrap();
        std::fs::remove_file(dir.join("apps/com.x/meta.json")).unwrap();
        let report = verify(&dir, |_, _| {}).unwrap();
        assert_eq!(report.problems.len(), 2);
        assert!(report.problems.iter().any(|p| p.problem == Problem::HashMismatch));
        assert!(report.problems.iter().any(|p| p.problem == Problem::Missing));

        let good = make_backup(&base, "B");
        let packed = pack(&good, None).unwrap();
        assert_eq!(verify(&packed, |_, _| {}).unwrap().problems, vec![]);
        let unpacked = unpack(&packed).unwrap();
        assert!(unpacked.join(MANIFEST).is_file() && !packed.exists());
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn verify_reports_corrupt_tar_with_matching_hash() {
        let base = tmp("corrupt");
        let dir = make_backup(&base, "A");
        // Rewrite the manifest entry to match a truncated archive: hash fine, tar broken.
        let tz = dir.join("apps/com.x/data/ce.tar.zst");
        let bytes = std::fs::read(&tz).unwrap();
        File::create(&tz).unwrap().write_all(&bytes[..bytes.len() - 8]).unwrap();
        let mut m = Manifest::read(&dir.join(MANIFEST)).unwrap();
        m.files[0].size = std::fs::metadata(&tz).unwrap().len();
        m.files[0].sha256 = sha256_file(&tz).unwrap();
        m.write(&dir).unwrap();
        let report = verify(&dir, |_, _| {}).unwrap();
        assert!(matches!(report.problems[0].problem, Problem::Corrupt { .. }), "{report:?}");
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn names_with_dots_pack_to_distinct_files() {
        let base = tmp("dots");
        make_backup(&base, "v1.2");
        make_backup(&base, "v1.3");
        let a = pack(&base.join("v1.2"), None).unwrap();
        let b = pack(&base.join("v1.3"), None).unwrap();
        assert_eq!(a.file_name().unwrap(), "v1.2.adbkeep");
        assert_eq!(b.file_name().unwrap(), "v1.3.adbkeep");
        assert!(a.is_file() && b.is_file());
        // Never overwrites an existing pack.
        make_backup(&base, "v1.2");
        assert!(pack(&base.join("v1.2"), None).is_err());
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn delete_refuses_non_backups() {
        let base = tmp("delete");
        std::fs::create_dir_all(base.join("photos")).unwrap();
        assert!(delete(&base.join("photos")).is_err());
        let dir = make_backup(&base, "A");
        delete(&dir).unwrap();
        assert!(!dir.exists());
        std::fs::remove_dir_all(&base).unwrap();
    }
}
