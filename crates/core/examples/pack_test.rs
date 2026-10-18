//! Packs a COPY of a backup with a password and exercises the encrypted archive paths.
//! `cargo run -p adbkeep-core --example pack_test -- <work-dir>` (expects <work-dir>/orig = a finished backup)

use std::path::{Path, PathBuf};

use adbkeep_core::{archive, library};

fn tree_hash(dir: &Path) -> Vec<(String, u64, String)> {
    let mut v = Vec::new();
    fn walk(base: &Path, d: &Path, v: &mut Vec<(String, u64, String)>) {
        for e in std::fs::read_dir(d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(base, &p, v);
            } else {
                let data = std::fs::read(&p).unwrap();
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                data.hash(&mut h);
                v.push((p.strip_prefix(base).unwrap().display().to_string(), data.len() as u64, format!("{:x}", h.finish())));
            }
        }
    }
    walk(dir, dir, &mut v);
    v.sort();
    v
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let work = PathBuf::from(std::env::args().nth(1).ok_or("work dir")?);
    let orig = work.join("orig");
    let before = tree_hash(&orig);
    for (label, pw) in [("plain", None), ("encrypted", Some("clave ñ 😀 'x'"))] {
        let dir = work.join(label);
        let _ = std::fs::remove_dir_all(&dir);
        copy_dir(&orig, &dir);
        let t = std::time::Instant::now();
        let packed = library::pack(&dir, pw)?;
        println!("[{label}] packed -> {} ({} bytes, {:?}) encrypted={}", packed.display(), std::fs::metadata(&packed)?.len(), t.elapsed(), archive::is_encrypted(&packed));
        if let Some(pw) = pw {
            archive::forget(&packed);
            println!("[{label}] list while locked: {:?}", library::list(&work).map(|l| l.iter().filter(|b| b.path.ends_with("encrypted.adbkeep")).map(|b| format!("{:?}", b.status)).collect::<Vec<_>>()));
            println!("[{label}] read_manifest without password -> {}", library::read_manifest(&packed).map(|_| "OK (BAD)".to_string()).unwrap_or_else(|e| format!("error: {e:?}")));
            println!("[{label}] wrong password -> {}", archive::unlock(&packed, "mala").map(|_| "OK (BAD)".to_string()).unwrap_or_else(|e| format!("error: {e:?}")));
            println!("[{label}] unlocked? {}", archive::is_unlocked(&packed));
            archive::unlock(&packed, pw)?;
            println!("[{label}] right password -> unlocked={}", archive::is_unlocked(&packed));
        }
        let r = library::verify(&packed, |_, _| {})?;
        println!("[{label}] verify: {} files, problems {:?}", r.files_checked, r.problems);
        let sms = library::read_file(&packed, "other/sms/sms.jsonl", usize::MAX)?;
        println!("[{label}] read sms.jsonl: {} bytes", sms.len());
        let out = library::unpack(&packed)?;
        let after = tree_hash(&out);
        println!("[{label}] unpacked identical to original: {}", after == before);
    }
    // corruption: flip a byte in a packed (plain) archive and verify must report it
    let dir = work.join("corrupt");
    let _ = std::fs::remove_dir_all(&dir);
    copy_dir(&orig, &dir);
    let packed = library::pack(&dir, None)?;
    let mut bytes = std::fs::read(&packed)?;
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xFF;
    std::fs::write(&packed, &bytes)?;
    println!("[corrupt packed] verify -> {}", match library::verify(&packed, |_, _| {}) { Ok(r) => format!("ok={} problems {:?}", r.problems.is_empty(), r.problems), Err(e) => format!("error: {e:?}") });
    // corruption in a folder backup: truncate a file
    let dir2 = work.join("corrupt2");
    let _ = std::fs::remove_dir_all(&dir2);
    copy_dir(&orig, &dir2);
    let f = dir2.join("apps/org.fdroid.fdroid/data/ce.tar.zst");
    let d = std::fs::read(&f)?;
    std::fs::write(&f, &d[..d.len() / 2])?;
    let g = dir2.join("other/sms/sms.jsonl");
    std::fs::remove_file(&g)?;
    let r = library::verify(&dir2, |_, _| {})?;
    println!("[corrupt folder] problems: {:?}", r.problems);
    Ok(())
}

fn copy_dir(a: &Path, b: &Path) {
    std::fs::create_dir_all(b).unwrap();
    for e in std::fs::read_dir(a).unwrap() {
        let e = e.unwrap();
        let (s, d) = (e.path(), b.join(e.file_name()));
        if s.is_dir() { copy_dir(&s, &d) } else { std::fs::copy(&s, &d).unwrap(); }
    }
}
