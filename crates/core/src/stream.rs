//! Streaming binary output of a root command on the phone into a local file.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::device::Device;
use crate::log::{LogEntry, capture, now_ms};
use crate::root::as_root_binary;
use crate::{Error, Result};

const CHUNK: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct StreamResult {
    /// Bytes received from the phone.
    pub bytes_in: u64,
    /// Bytes written to the file (after compression).
    pub bytes_out: u64,
    /// SHA-256 of the file, lowercase hex.
    pub sha256: String,
}

/// Runs `cmd` as root and writes its stdout to `dest`, zstd-compressed when `zstd_level` is set.
/// Uses `adb shell -T` (no pty, shell protocol v2) so the remote exit code is known; see
/// [`as_root_binary`]. Writes to `<dest>.partial` and renames on success.
/// `on_progress` gets the bytes received so far.
pub async fn root_to_file(
    device: &Device,
    cmd: &str,
    dest: &Path,
    zstd_level: Option<i32>,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(u64),
) -> Result<StreamResult> {
    let method = device.root().ok_or(Error::RootUnavailable)?;
    let remote = as_root_binary(method, cmd);
    let args = ["-s", device.serial(), "shell", "-T", remote.as_str()];
    let log = device.adb().log().clone();
    let id = log.next_id();
    let started_at = now_ms();
    let start = Instant::now();

    let partial = partial_path(dest);
    let result = async {
        let mut child = device
            .adb()
            .command()
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Error::Spawn { program: device.adb().path().display().to_string(), reason: e.to_string() })?;
        let mut stdout = child.stdout.take().expect("piped");
        let mut stderr = child.stderr.take().expect("piped");
        let stderr_task = tokio::spawn(async move {
            let mut buf = Vec::new();
            let _ = stderr.read_to_end(&mut buf).await;
            buf
        });

        let file = HashWriter::new(BufWriter::with_capacity(CHUNK, File::create(&partial)?));
        let mut sink = match zstd_level {
            Some(level) => {
                let mut enc = zstd::Encoder::new(file, level)?;
                enc.include_checksum(true)?;
                Sink::Zstd(enc)
            }
            None => Sink::Plain(file),
        };
        let mut buf = vec![0u8; CHUNK];
        let mut bytes_in = 0u64;
        loop {
            if cancel.load(Ordering::Relaxed) {
                let _ = child.kill().await;
                return Err(Error::Cancelled);
            }
            let n = stdout.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            sink.write_all(&buf[..n])?;
            bytes_in += n as u64;
            on_progress(bytes_in);
        }
        let file = sink.finish()?;
        let (bytes_out, sha256) = file.finish()?;
        let status = child.wait().await?;
        let stderr = stderr_task.await.unwrap_or_default();
        Ok((status.code(), stderr, StreamResult { bytes_in, bytes_out, sha256 }))
    }
    .await;

    let (exit_code, stderr, streamed) = match &result {
        Ok((code, stderr, r)) => (*code, capture(stderr), Some(r.bytes_in)),
        Err(e) => (None, e.to_string(), None),
    };
    log.record(LogEntry {
        id,
        started_at,
        duration_ms: start.elapsed().as_millis() as u32,
        program: "adb".into(),
        args: args.iter().map(|s| s.to_string()).collect(),
        exit_code,
        stdout: String::new(),
        stderr,
        streamed_bytes: streamed,
        tag: device.adb().tag(),
    });

    match result {
        Ok((Some(0), _, r)) => {
            std::fs::rename(&partial, dest)?;
            Ok(r)
        }
        Ok((code, stderr, _)) => {
            let _ = std::fs::remove_file(&partial);
            Err(Error::CommandFailed { exit_code: code, stderr: capture(&stderr) })
        }
        Err(e) => {
            let _ = std::fs::remove_file(&partial);
            Err(e)
        }
    }
}

/// Writes a local file to `remote` on the phone as the adb shell user (no root, no pty:
/// `su`'s pty would mangle binary stdin), zstd-decompressing it first when `decompress`.
/// `on_progress` gets the bytes read from the local file.
pub async fn file_to_device(
    device: &Device,
    local: &Path,
    decompress: bool,
    remote: &str,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(u64),
) -> Result<u64> {
    let remote_cmd = format!("cat > {}", crate::root::shell_quote(remote));
    let args = ["-s", device.serial(), "shell", "-T", remote_cmd.as_str()];
    let log = device.adb().log().clone();
    let id = log.next_id();
    let started_at = now_ms();
    let start = Instant::now();

    let result = async {
        let mut child = device
            .adb()
            .command()
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Error::Spawn { program: device.adb().path().display().to_string(), reason: e.to_string() })?;
        let mut stdin = child.stdin.take().expect("piped");
        let mut stderr = child.stderr.take().expect("piped");
        let stderr_task = tokio::spawn(async move {
            let mut buf = Vec::new();
            let _ = stderr.read_to_end(&mut buf).await;
            buf
        });
        let counter = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let file = CountingReader { inner: std::io::BufReader::new(File::open(local)?), count: counter.clone() };
        let mut source: Box<dyn std::io::Read + Send> =
            if decompress { Box::new(zstd::Decoder::new(file)?) } else { Box::new(file) };
        let mut buf = vec![0u8; CHUNK];
        let mut written = 0u64;
        loop {
            if cancel.load(Ordering::Relaxed) {
                let _ = child.kill().await;
                return Err(Error::Cancelled);
            }
            let n = std::io::Read::read(&mut source, &mut buf)?;
            if n == 0 {
                break;
            }
            stdin.write_all(&buf[..n]).await?;
            written += n as u64;
            on_progress(counter.load(Ordering::Relaxed));
        }
        stdin.shutdown().await?;
        drop(stdin);
        let status = child.wait().await?;
        let stderr = stderr_task.await.unwrap_or_default();
        Ok((status.code(), stderr, written))
    }
    .await;

    let (exit_code, stderr) = match &result {
        Ok((code, stderr, _)) => (*code, capture(stderr)),
        Err(e) => (None, e.to_string()),
    };
    log.record(LogEntry {
        id,
        started_at,
        duration_ms: start.elapsed().as_millis() as u32,
        program: "adb".into(),
        args: args.iter().map(|s| s.to_string()).chain([format!("< {}", local.display())]).collect(),
        exit_code,
        stdout: String::new(),
        stderr,
        streamed_bytes: result.as_ref().ok().map(|r| r.2),
        tag: device.adb().tag(),
    });
    match result? {
        (Some(0), _, written) => Ok(written),
        (code, stderr, _) => Err(Error::CommandFailed { exit_code: code, stderr: capture(&stderr) }),
    }
}

struct CountingReader<R> {
    inner: R,
    count: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl<R: std::io::Read> std::io::Read for CountingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.count.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}

fn partial_path(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(".partial");
    dest.with_file_name(name)
}

enum Sink<W: Write> {
    Plain(W),
    Zstd(zstd::Encoder<'static, W>),
}

impl<W: Write> Sink<W> {
    fn write_all(&mut self, data: &[u8]) -> std::io::Result<()> {
        match self {
            Sink::Plain(w) => w.write_all(data),
            Sink::Zstd(w) => w.write_all(data),
        }
    }

    fn finish(self) -> std::io::Result<W> {
        match self {
            Sink::Plain(w) => Ok(w),
            Sink::Zstd(w) => w.finish(),
        }
    }
}

/// Counts and hashes what goes through it.
pub struct HashWriter<W: Write> {
    inner: W,
    hasher: Sha256,
    bytes: u64,
}

impl<W: Write> HashWriter<W> {
    pub fn new(inner: W) -> Self {
        Self { inner, hasher: Sha256::new(), bytes: 0 }
    }

    /// Flushes and returns (bytes written, SHA-256 hex).
    pub fn finish(mut self) -> std::io::Result<(u64, String)> {
        self.inner.flush()?;
        Ok((self.bytes, hex(&self.hasher.finalize())))
    }
}

impl<W: Write> Write for HashWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        self.bytes += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// SHA-256 of a local file, lowercase hex.
pub fn sha256_file(path: &Path) -> Result<String> {
    let mut w = HashWriter::new(std::io::sink());
    std::io::copy(&mut File::open(path)?, &mut w)?;
    Ok(w.finish()?.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_writer_matches_known_digest() {
        let mut w = HashWriter::new(Vec::new());
        w.write_all(b"abc").unwrap();
        let (n, sha) = w.finish().unwrap();
        assert_eq!(n, 3);
        assert_eq!(sha, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
