//! `adb track-devices -l`: the adb server pushes the device list on every change.

use std::process::Stdio;

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, ChildStdout};

use crate::adb::{Adb, DeviceEntry, parse_devices};
use crate::{Error, Result};

/// Starts `adb track-devices -l`; read updates from it with [`next_update`].
pub fn spawn(adb: &Adb) -> Result<(Child, ChildStdout)> {
    let mut child = adb
        .command()
        .args(["track-devices", "-l"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| Error::Spawn { program: adb.path().display().to_string(), reason: e.to_string() })?;
    let stdout = child.stdout.take().expect("stdout is piped");
    Ok((child, stdout))
}

/// Reads one update: 4 hex digits with the payload length, then `devices -l` lines.
/// `Ok(None)` when the stream ends (adb server stopped).
pub async fn next_update<R: AsyncRead + Unpin>(r: &mut R) -> Result<Option<Vec<DeviceEntry>>> {
    let mut len = [0u8; 4];
    match r.read_exact(&mut len).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let len = std::str::from_utf8(&len)
        .ok()
        .and_then(|s| usize::from_str_radix(s, 16).ok())
        .ok_or_else(|| Error::UnexpectedOutput { what: "adb track-devices".into() })?;
    let mut payload = vec![0u8; len];
    r.read_exact(&mut payload).await?;
    Ok(Some(parse_devices(&String::from_utf8_lossy(&payload))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adb::DeviceState;

    #[tokio::test]
    async fn reads_framed_updates() {
        let one = "59021FDCH001KW         device usb:1-11 product:blazer model:Pixel_10_Pro device:blazer transport_id:7\n";
        let stream = format!("{:04x}{one}0000", one.len());
        let mut r = stream.as_bytes();
        let first = next_update(&mut r).await.unwrap().unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].state, DeviceState::Device);
        assert_eq!(first[0].model.as_deref(), Some("Pixel_10_Pro"));
        assert_eq!(next_update(&mut r).await.unwrap().unwrap(), vec![]);
        assert_eq!(next_update(&mut r).await.unwrap(), None);
    }
}
