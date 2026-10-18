//! Packing a backup folder into one `.adbkeep` file (tar, zstd, and optionally age with a
//! password) and reading it back.
//!
//! An encrypted pack is a standard age file, so it can be opened without adbkeep:
//! `age -d copia.adbkeep | zstd -d | tar -x`.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use age::secrecy::{ExposeSecret, SecretString};
use age_core::format::{FileKey, Stanza};
use sha2::{Digest, Sha256};

use crate::format::{MANIFEST, Manifest};
use crate::{Error, Result};

/// zstd level for the outer archive: the data inside is already compressed.
const PACK_LEVEL: i32 = 1;
/// First bytes of every age file.
const AGE_MAGIC: &[u8] = b"age-encryption.org/v1";

/// Passwords given this session, by pack path. Never written to disk.
static PASSWORDS: LazyLock<Mutex<HashMap<PathBuf, String>>> = LazyLock::new(Default::default);
/// File keys already unwrapped, by scrypt stanza (salt, work factor and wrapped key), so the
/// deliberately slow scrypt runs once per pack and session instead of on every read.
static FILE_KEYS: LazyLock<Mutex<HashMap<StanzaKey, [u8; 16]>>> = LazyLock::new(Default::default);
/// Arguments (salt, work factor), body (wrapped file key) of an scrypt stanza and the SHA-256 of
/// the password it was unwrapped with, so a wrong password never hits the cache.
type StanzaKey = (Vec<String>, Vec<u8>, [u8; 32]);

fn key_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Whether `path` is a pack encrypted with a password.
pub fn is_encrypted(path: &Path) -> bool {
    let mut magic = [0u8; AGE_MAGIC.len()];
    File::open(path).and_then(|mut f| f.read_exact(&mut magic)).is_ok() && magic == AGE_MAGIC
}

/// Whether the password of an encrypted pack is known this session.
pub fn is_unlocked(path: &Path) -> bool {
    PASSWORDS.lock().unwrap().contains_key(&key_path(path))
}

/// Remembers `password` for the encrypted pack at `path` if it is right; returns its manifest.
pub fn unlock(path: &Path, password: &str) -> Result<Manifest> {
    let key = key_path(path);
    let previous = PASSWORDS.lock().unwrap().insert(key.clone(), password.to_string());
    let result = read_manifest(path);
    if result.is_err() {
        let mut passwords = PASSWORDS.lock().unwrap();
        match previous {
            Some(p) => passwords.insert(key, p),
            None => passwords.remove(&key),
        };
    }
    result
}

/// Forgets the password of a pack (when it is deleted or unpacked).
pub fn forget(path: &Path) {
    PASSWORDS.lock().unwrap().remove(&key_path(path));
    FILE_KEYS.lock().unwrap().clear();
}

fn remember(path: &Path, password: &str) {
    PASSWORDS.lock().unwrap().insert(key_path(path), password.to_string());
}

/// The scrypt identity for one password, with the file-key cache in front of it.
struct CachedIdentity(SecretString);

impl age::Identity for CachedIdentity {
    fn unwrap_stanza(&self, stanza: &Stanza) -> Option<std::result::Result<FileKey, age::DecryptError>> {
        let password: [u8; 32] = Sha256::digest(self.0.expose_secret().as_bytes()).into();
        let cache_key = (stanza.args.clone(), stanza.body.clone(), password);
        if let Some(bytes) = FILE_KEYS.lock().unwrap().get(&cache_key) {
            return Some(Ok(FileKey::new(Box::new(*bytes))));
        }
        let result = age::scrypt::Identity::new(self.0.clone()).unwrap_stanza(stanza)?;
        if let Ok(key) = &result {
            FILE_KEYS.lock().unwrap().insert(cache_key, *key.expose_secret());
        }
        Some(result)
    }
}

fn decrypt_error(e: age::DecryptError) -> Error {
    match e {
        age::DecryptError::DecryptionFailed | age::DecryptError::KeyDecryptionFailed | age::DecryptError::NoMatchingKeys
        | age::DecryptError::InvalidMac => Error::WrongPassword,
        e => Error::Io { reason: e.to_string() },
    }
}

/// The tar stream inside a pack: decrypted (with the remembered password) and decompressed.
pub fn open(path: &Path) -> Result<tar::Archive<Box<dyn Read>>> {
    let file = BufReader::new(File::open(path)?);
    let raw: Box<dyn Read> = if is_encrypted(path) {
        let password = PASSWORDS.lock().unwrap().get(&key_path(path)).cloned().ok_or(Error::PasswordRequired)?;
        let identity = CachedIdentity(SecretString::from(password));
        let decryptor = age::Decryptor::new_buffered(file).map_err(decrypt_error)?;
        Box::new(decryptor.decrypt(std::iter::once(&identity as &dyn age::Identity)).map_err(decrypt_error)?)
    } else {
        Box::new(file)
    };
    Ok(tar::Archive::new(Box::new(zstd::Decoder::new(raw)?)))
}

/// Writes `dir` into `dest` with `manifest.json` as the first entry, so it can be read
/// without unpacking (see [`read_manifest`]). With a password the pack is age-encrypted, and
/// the password is remembered for this session.
pub fn pack(dir: &Path, dest: &Path, password: Option<&str>) -> Result<()> {
    if dest.exists() {
        return Err(Error::Io { reason: format!("{} already exists", dest.display()) });
    }
    let partial = crate::format::with_suffix(dest, "partial");
    let result = (|| -> Result<()> {
        let out = BufWriter::new(File::create(&partial)?);
        let sink: Box<dyn FinishWrite> = match password {
            Some(p) => {
                let enc = age::Encryptor::with_user_passphrase(SecretString::from(p.to_string()));
                Box::new(enc.wrap_output(out)?)
            }
            None => Box::new(out),
        };
        let enc = zstd::Encoder::new(sink, PACK_LEVEL)?;
        let mut tar = tar::Builder::new(enc);
        tar.follow_symlinks(false);
        tar.append_path_with_name(dir.join(MANIFEST), MANIFEST)?;
        let mut files = walk(dir)?;
        // Metadata before the bulky tars and APKs, so the viewers in Mis copias reach it
        // without decompressing everything behind it.
        files.sort_by_key(|p| p.extension().is_some_and(|e| e == "zst" || e == "apk"));
        for entry in files {
            let rel = entry.strip_prefix(dir).expect("inside dir");
            if rel == Path::new(MANIFEST) {
                continue;
            }
            tar.append_path_with_name(&entry, rel)?;
        }
        tar.into_inner()?.finish()?.finish_write()?;
        Ok(())
    })();
    match result {
        Ok(()) => {
            // Reach the disk before the rename makes it the real file.
            std::fs::OpenOptions::new().write(true).open(&partial).and_then(|f| f.sync_all())?;
            std::fs::rename(&partial, dest)?;
            if let Some(p) = password {
                remember(dest, p);
            }
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&partial);
            Err(e)
        }
    }
}

/// A writer whose end must be written explicitly (age's last chunk).
trait FinishWrite: Write {
    fn finish_write(self: Box<Self>) -> std::io::Result<()>;
}

impl<W: Write> FinishWrite for BufWriter<W> {
    fn finish_write(mut self: Box<Self>) -> std::io::Result<()> {
        self.flush()
    }
}

impl<W: Write> FinishWrite for age::stream::StreamWriter<W> {
    fn finish_write(self: Box<Self>) -> std::io::Result<()> {
        (*self).finish()?.flush()
    }
}

/// Reads `manifest.json` from the start of a packed backup.
pub fn read_manifest(packed: &Path) -> Result<Manifest> {
    let mut tar = open(packed)?;
    let mut entry = tar
        .entries()?
        .next()
        .ok_or_else(|| Error::UnexpectedOutput { what: packed.display().to_string() })??;
    if entry.path()?.as_ref() != Path::new(MANIFEST) {
        return Err(Error::UnexpectedOutput { what: format!("{}: no {MANIFEST} first", packed.display()) });
    }
    let mut text = String::new();
    entry.read_to_string(&mut text)?;
    Manifest::parse(&text)
}

/// Unpacks into `dest` (created).
pub fn unpack(packed: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    open(packed)?.unpack(dest)?;
    Ok(())
}

/// Files under `dir`, sorted, recursively.
fn walk(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{FORMAT_VERSION, SourceDevice};

    #[test]
    fn pack_read_manifest_and_unpack() {
        let base = std::env::temp_dir().join(format!("adbkeep-archive-{}", std::process::id()));
        let dir = base.join("Pixel_2026-10-14_1400");
        std::fs::create_dir_all(dir.join("apps/com.x/data")).unwrap();
        std::fs::write(dir.join("apps/com.x/data/ce.tar.zst"), b"data").unwrap();
        let manifest = Manifest {
            format_version: FORMAT_VERSION,
            app_version: "0.1.0".into(),
            created_at: "2026-10-14T12:00:00Z".into(),
            name: "Pixel_2026-10-14_1400".into(),
            source: SourceDevice {
                serial: "s".into(),
                manufacturer: "Google".into(),
                model: "Pixel 10 Pro".into(),
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
            files: vec![],
            errors: vec![],
            complete: true,
        };
        manifest.write(&dir).unwrap();
        let packed = base.join("Pixel_2026-10-14_1400.adbkeep");
        pack(&dir, &packed, None).unwrap();
        assert_eq!(read_manifest(&packed).unwrap(), manifest);
        let out = base.join("out");
        unpack(&packed, &out).unwrap();
        assert_eq!(std::fs::read(out.join("apps/com.x/data/ce.tar.zst")).unwrap(), b"data");
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn encrypted_pack_needs_the_password() {
        let base = std::env::temp_dir().join(format!("adbkeep-archive-enc-{}", std::process::id()));
        let dir = base.join("A");
        std::fs::create_dir_all(dir.join("apps/com.x")).unwrap();
        std::fs::write(dir.join("apps/com.x/meta.json"), b"{}").unwrap();
        std::fs::write(dir.join(MANIFEST), serde_json::to_string(&serde_json::json!({
            "formatVersion": FORMAT_VERSION, "appVersion": "0", "createdAt": "2026-10-15T00:00:00Z", "name": "A",
            "source": {"serial": "s", "manufacturer": "", "model": "", "androidRelease": "16", "sdk": 36, "abis": [],
                       "buildType": "user", "buildId": "", "rootMethod": null},
            "apps": [], "accounts": [], "keystorePackages": [], "files": [], "errors": [], "complete": true
        })).unwrap()).unwrap();
        let packed = base.join("A.adbkeep");
        pack(&dir, &packed, Some("secreto")).unwrap();
        assert!(is_encrypted(&packed));
        // Remembered right after packing; forget it to check the locked path.
        assert!(is_unlocked(&packed));
        forget(&packed);
        assert_eq!(read_manifest(&packed).unwrap_err(), Error::PasswordRequired);
        assert_eq!(unlock(&packed, "otra").unwrap_err(), Error::WrongPassword);
        assert!(!is_unlocked(&packed));
        assert_eq!(unlock(&packed, "secreto").unwrap().name, "A");
        // The cached file key must not make a wrong password work.
        forget(&packed);
        assert_eq!(unlock(&packed, "otra").unwrap_err(), Error::WrongPassword);
        assert_eq!(unlock(&packed, "secreto").unwrap().name, "A");
        let out = base.join("out");
        unpack(&packed, &out).unwrap();
        assert_eq!(std::fs::read(out.join("apps/com.x/meta.json")).unwrap(), b"{}");
        std::fs::remove_dir_all(&base).unwrap();
    }
}
