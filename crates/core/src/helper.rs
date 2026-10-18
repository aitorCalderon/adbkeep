//! The on-device helper (helper/ in the repo): a dex run with `app_process` as root that reads
//! app names and icons, and content providers through the same API the `content` command uses,
//! with output that can be parsed reliably (backup-android.md, "Extracción fiable", option 3).

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use serde::Deserialize;
use serde_json::{Map, Value};

use crate::device::Device;
use crate::restore::REMOTE_TMP;
use crate::root::shell_quote;
use crate::stream::{StreamResult, file_to_device, root_to_file};
use crate::{Error, Result};

/// Must match `Main.PROTOCOL` in helper/src/dev/adbkeep/helper/Main.java.
pub const PROTOCOL: u32 = 4;
pub const DEX_NAME: &str = "adbkeep-helper.dex";

/// Each deployment gets its own file, so two never replace each other's dex while it runs
/// (app_process aborts with exit code 134 when that happens).
fn remote_dex(id: u64) -> String {
    format!("{REMOTE_TMP}/helper-{id}.dex")
}

fn next_id() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::process::id() as u64 * 1000 + n
}

/// The command line that runs the helper deployed at `dex` with `args`.
pub fn command(dex: &str, args: &[&str]) -> String {
    let quoted: Vec<String> = args.iter().map(|a| shell_quote(a)).collect();
    format!("CLASSPATH={dex} app_process /system/bin dev.adbkeep.helper.Main {}", quoted.join(" "))
}

/// A deployed helper. Writes only to `/data/local/tmp/adbkeep`; [`Helper::remove`] cleans up.
pub struct Helper<'a> {
    device: &'a Device,
    /// Path of this deployment's dex on the phone.
    remote: String,
    /// Local scratch directory for outputs.
    tmp: std::path::PathBuf,
}

/// Result of an insert command: rows written, rows that failed and the first errors.
#[derive(Debug, Clone, PartialEq, Default, Deserialize, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct InsertSummary {
    pub inserted: u32,
    pub failed: u32,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct BatchQuery<'a> {
    pub uri: &'a str,
    pub projection: Option<&'a str>,
    pub selection: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AppLabel {
    pub package: String,
    pub label: Option<String>,
    /// PNG, base64.
    pub icon: Option<String>,
    pub error: Option<String>,
}

impl<'a> Helper<'a> {
    /// Pushes the dex (read-only) and checks that it runs and speaks our protocol.
    pub async fn deploy(device: &'a Device, dex: &Path, tmp: &Path) -> Result<Helper<'a>> {
        std::fs::create_dir_all(tmp)?;
        let never = AtomicBool::new(false);
        let remote = remote_dex(next_id());
        device.shell(&format!("mkdir -p {REMOTE_TMP} && rm -f {remote}")).await?.ok()?;
        file_to_device(device, dex, false, &remote, &never, |_| {}).await?;
        device.shell(&format!("chmod 444 {remote}")).await?.ok()?;
        let helper = Helper { device, remote, tmp: tmp.to_path_buf() };
        let out = helper.run_text(&["version"]).await?;
        #[derive(Deserialize)]
        struct Version {
            protocol: u32,
        }
        let v: Version = out
            .lines()
            .find_map(|l| serde_json::from_str(l).ok())
            .ok_or_else(|| Error::UnexpectedOutput { what: "helper version".into() })?;
        if v.protocol != PROTOCOL {
            return Err(Error::UnexpectedOutput { what: format!("helper protocol {}", v.protocol) });
        }
        Ok(helper)
    }

    /// Runs the helper as root with stdout streamed to a local file (binary-safe, exit code kept).
    pub async fn run_to_file(&self, args: &[&str], dest: &Path) -> Result<StreamResult> {
        let never = AtomicBool::new(false);
        root_to_file(self.device, &command(&self.remote, args), dest, None, &never, |_| {}).await
    }

    async fn run_text(&self, args: &[&str]) -> Result<String> {
        let dest = self.tmp.join(format!("{}.out", self.remote.rsplit('/').next().unwrap_or("helper")));
        self.run_to_file(args, &dest).await?;
        let text = std::fs::read_to_string(&dest)?;
        let _ = std::fs::remove_file(&dest);
        Ok(text)
    }

    /// Name and icon of each package.
    pub async fn labels(&self, packages: &[String]) -> Result<HashMap<String, AppLabel>> {
        let mut args = vec!["labels"];
        args.extend(packages.iter().map(String::as_str));
        let text = self.run_text(&args).await?;
        Ok(parse_lines::<AppLabel>(&text).into_iter().map(|l| (l.package.clone(), l)).collect())
    }

    /// Rows of a content provider query, with typed values (blobs as `{"base64": …}`).
    pub async fn query(&self, uri: &str, projection: Option<&[&str]>, selection: Option<&str>, sort: Option<&str>) -> Result<Vec<Map<String, Value>>> {
        let joined = projection.map(|p| p.join(","));
        let mut args = vec!["query", uri];
        if let Some(p) = &joined {
            args.extend(["--projection", p.as_str()]);
        }
        if let Some(s) = selection {
            args.extend(["--where", s]);
        }
        if let Some(s) = sort {
            args.extend(["--sort", s]);
        }
        Ok(parse_lines(&self.run_text(&args).await?))
    }

    /// Several queries in one helper start. `projection` "#count" gives only `{"count": n}`.
    /// Returns, per query, its rows or the error message.
    pub async fn batch(&self, queries: &[BatchQuery<'_>]) -> Result<Vec<std::result::Result<Vec<Map<String, Value>>, String>>> {
        let mut args: Vec<&str> = vec!["batch"];
        for q in queries {
            args.extend([q.uri, q.projection.unwrap_or("-"), q.selection.unwrap_or("-")]);
        }
        let mut out: Vec<std::result::Result<Vec<Map<String, Value>>, String>> = vec![Ok(vec![]); queries.len()];
        for mut row in parse_lines::<Map<String, Value>>(&self.run_text(&args).await?) {
            let Some(q) = row.remove("_q").and_then(|v| v.as_u64()).map(|q| q as usize).filter(|q| *q < out.len()) else {
                continue;
            };
            if let Some(Value::String(e)) = row.get("error") {
                out[q] = Err(e.clone());
            } else if let Ok(rows) = &mut out[q] {
                rows.push(row);
            }
        }
        Ok(out)
    }

    /// Runs a query and writes the JSON lines straight to `dest` (large tables).
    pub async fn query_to_file(&self, uri: &str, selection: Option<&str>, dest: &Path) -> Result<u64> {
        let mut args = vec!["query", uri];
        if let Some(s) = selection {
            args.extend(["--where", s]);
        }
        self.run_to_file(&args, dest).await?;
        count_lines(dest)
    }

    /// `template` with `{id}` replaced by each id (e.g. `content://mms/{id}/addr`), into `dest`.
    /// Rows get `_id_of`. Ids are sent in batches to keep the command line short.
    pub async fn query_each_to_file(&self, template: &str, ids: &[String], dest: &Path) -> Result<u64> {
        use std::io::Write;
        let mut out = std::fs::File::create(dest)?;
        let part = dest.with_extension("part");
        for chunk in ids.chunks(200) {
            let joined = chunk.join(",");
            self.run_to_file(&["query-each", template, &joined], &part).await?;
            out.write_all(&std::fs::read(&part)?)?;
        }
        let _ = std::fs::remove_file(&part);
        drop(out);
        count_lines(dest)
    }

    /// Runs an insert command (`insert`, `insert-contacts`, `insert-calendar`) whose input files
    /// are already on the phone; returns its summary line.
    pub async fn insert(&self, args: &[&str]) -> Result<InsertSummary> {
        let text = self.run_text(args).await?;
        parse_lines::<InsertSummary>(&text)
            .into_iter()
            .next()
            .ok_or_else(|| Error::UnexpectedOutput { what: format!("helper {}", args.first().unwrap_or(&"")) })
    }

    /// What `openAssetFile(uri)` returns (vCards, MMS parts), into `dest`.
    pub async fn asset_to_file(&self, uri: &str, dest: &Path) -> Result<StreamResult> {
        self.run_to_file(&["asset", uri], dest).await
    }

    /// Deletes the dex, and the scratch folder if nothing else is left in it.
    pub async fn remove(self) -> Result<()> {
        self.device.shell(&format!("rm -f {}; rmdir {REMOTE_TMP} 2>/dev/null; true", self.remote)).await?;
        Ok(())
    }
}

/// Number of JSON rows in a helper output file.
pub fn count_lines(path: &Path) -> Result<u64> {
    use std::io::BufRead;
    let file = std::io::BufReader::new(std::fs::File::open(path)?);
    Ok(file.lines().map_while(|l| l.ok()).filter(|l| l.starts_with('{')).count() as u64)
}

/// JSON objects, one per line; anything else (e.g. runtime warnings) is skipped.
pub fn parse_lines<T: for<'de> Deserialize<'de>>(text: &str) -> Vec<T> {
    text.lines().filter(|l| l.starts_with('{')).filter_map(|l| serde_json::from_str(l).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_command_line() {
        assert_eq!(
            command("/data/local/tmp/adbkeep/helper-1.dex", &["query", "content://sms", "--where", "type = 1"]),
            "CLASSPATH=/data/local/tmp/adbkeep/helper-1.dex app_process /system/bin dev.adbkeep.helper.Main query content://sms --where 'type = 1'"
        );
    }

    #[test]
    fn parses_json_lines_and_skips_noise() {
        let text = "WARNING: linker: something\n{\"package\":\"com.x\",\"label\":\"X, \\\"quoted\\\"\\nline\",\"icon\":\"iVBO\"}\n{\"package\":\"com.y\",\"error\":\"not installed\"}\n";
        let labels: Vec<AppLabel> = parse_lines(text);
        assert_eq!(labels.len(), 2);
        assert_eq!(labels[0].label.as_deref(), Some("X, \"quoted\"\nline"));
        assert_eq!(labels[1].error.as_deref(), Some("not installed"));
    }
}
