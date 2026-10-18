//! Building device-side shell commands that run as root.

use serde::{Deserialize, Serialize};

/// How root is obtained on the phone (backup-android.md, Contexto).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum RootMethod {
    /// `su` from Magisk, KernelSU…: commands are wrapped in `su -c '…'`.
    Su,
    /// adbd itself runs as root (`adb root`): commands run as-is.
    AdbRoot,
}

/// Quotes `s` as a single POSIX shell word.
pub fn shell_quote(s: &str) -> String {
    if !s.is_empty()
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-./:=@%+,".contains(&b))
    {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', r#"'\''"#))
}

/// Wraps a device shell command so it runs as root.
pub fn as_root(method: RootMethod, cmd: &str) -> String {
    match method {
        RootMethod::Su => format!("su -c {}", shell_quote(cmd)),
        RootMethod::AdbRoot => cmd.to_string(),
    }
}

/// Like [`as_root`], for commands whose stdout is binary and read with `adb exec-out`.
/// - Magisk's `su` allocates its own pty, which turns `\n` into `\r\n`; piping through
///   `cat` keeps the stream intact (backup-android.md, "Transporte binario seguro").
/// - `exec-out` and the `su` pty both mix stderr into stdout, so stderr is discarded
///   (seen: "tar: had errors" inside the stream).
/// - `pipefail` keeps the command's exit code, which `| cat` would otherwise hide.
pub fn as_root_binary(method: RootMethod, cmd: &str) -> String {
    format!("set -o pipefail; {} | cat", as_root(method, &format!("{cmd} 2>/dev/null")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_only_when_needed() {
        assert_eq!(shell_quote("com.example"), "com.example");
        assert_eq!(shell_quote("/data/data"), "/data/data");
        assert_eq!(shell_quote(""), "''");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("it's"), r#"'it'\''s'"#);
    }

    #[test]
    fn wraps_su_and_adb_root() {
        let cmd = "tar -cpf - -C /data/data com.example";
        assert_eq!(
            as_root_binary(RootMethod::Su, cmd),
            "set -o pipefail; su -c 'tar -cpf - -C /data/data com.example 2>/dev/null' | cat"
        );
        assert_eq!(
            as_root_binary(RootMethod::AdbRoot, cmd),
            format!("set -o pipefail; {cmd} 2>/dev/null | cat")
        );
    }

    #[test]
    fn nested_quotes_survive_su() {
        assert_eq!(
            as_root(RootMethod::Su, "echo 'hi'"),
            r#"su -c 'echo '\''hi'\'''"#
        );
    }
}
