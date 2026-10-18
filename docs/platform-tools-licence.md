# Redistributing adb (platform-tools) with adbkeep

Reviewed on 2026-10-15 with platform-tools 37.0.1. This is not legal advice.

## What gets bundled
`scripts/fetch-platform-tools.mjs` downloads `platform-tools-latest-<os>.zip` from `dl.google.com`
and keeps only `adb` (`adb.exe`, `AdbWinApi.dll` and `AdbWinUsbApi.dll` on Windows), `NOTICE.txt` and
`source.properties`. The installer ships those files in `platform-tools/`.

## What the licences say
- The download page requires accepting the **Android Software Development Kit License Agreement**.
  - **§3.4** forbids copying (except for backup), modifying or **redistributing** the SDK or any
    part of it, "except to the extent required by applicable third party licenses".
  - **§3.5** says that the use and distribution of open-source-licensed SDK components is governed
    *only* by that licence, not by the agreement.
- The adb code comes from AOSP (`packages/modules/adb`) under the **Apache 2.0** licence. AdbWinApi and
  AdbWinUsbApi are also in AOSP under Apache 2.0. The zip's `NOTICE.txt` lists those licences
  (Apache 2.0, BSD, etc.).

## Conclusion
- Under §3.5, redistributing the adb binaries appears to be allowed as an open-source component,
  provided `NOTICE.txt` is included (Apache 2.0 §4) and notices are not altered (§3.8). Other
  projects do the same; for example, scrcpy ships adb in its Windows releases.
- Risk: the binaries are built by Google and downloaded under the SDK agreement, and §3.4
  is broad. This is a grey area, not an explicit permission.

## Options if you want to avoid the risk
1. **Don't bundle adb** and download it on first launch from `dl.google.com`, showing the
   agreement for the user to accept. Preferences already lets users choose a different adb.
2. **Build adb from the AOSP source** in CI. This is the cleanest legally, but it is costly
   and has to be maintained.
3. **Use the system adb** on Linux (the distribution's `adb` or `android-tools` package) and
   bundle it only on Windows and macOS.

Until a decision is made, CI keeps bundling the downloaded adb with its `NOTICE.txt`, and
`src-tauri/resources/platform-tools/` is not committed to the repository (`.gitignore`).
