# adbkeep

Desktop app to **back up an Android phone and restore it onto another one**, possibly running a different Android version. It works over USB with root access through ADB (Magisk/KernelSU `su` or `adb root`).

> **Status:** under development (v0.1.0). Only tested on Linux. Supports Android 11 and later.

## Features

- **Backup and restore on a single screen**, with tabs (User apps · System apps · Everything else · Warnings) and a fixed action bar.
- **Per-app selection**: choose which apps are backed up or restored and whether their data is included.
- **Simple and advanced modes**: advanced mode adds technical detail and fine-grained control without changing the flow.
- **Portability across devices and versions**:
  - App data: raw copy with UID and SELinux fix-up.
  - System data: logical export through the target system's API.
- Covers APKs, app data, permissions and AppOps, contacts, call log, SMS/MMS, Wi-Fi, settings, shared storage (`/sdcard`), blocked numbers, notifications and more.
- Warnings about what cannot be migrated (accounts, apps that use the Android Keystore, etc.).
- Interface in Spanish and English.
- Installers for Linux (AppImage, `.deb`, `.rpm`), macOS (Apple silicon and Intel) and Windows (`.msi`/`.exe`), with `adb` included.

Details on what is backed up and how each part works are in [`backup-android.md`](backup-android.md). The UI specification is in [`ui-spec.md`](ui-spec.md).

## Requirements

- A phone running **Android 11 or later**, with **USB debugging** enabled and **root access** through ADB.
- A computer running Linux. macOS and Windows builds are produced by CI but have not been tested.
- To develop: [Node.js](https://nodejs.org/) 22, [pnpm](https://pnpm.io/), [Rust](https://rustup.rs/) (2024 edition) and the [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your system.
- To build the on-device helper: JDK 17 and the Android SDK (platform 36, build-tools 36.0.0).

## Development

```sh
pnpm install
pnpm fetch-adb      # download Google's platform-tools for your system
pnpm build-helper   # build the helper that runs on the phone
pnpm tauri dev      # start the app in development mode
```

Other useful scripts:

| Command | What it does |
|---|---|
| `pnpm tauri build` | Build the installers |
| `pnpm test` | Frontend tests (Vitest) |
| `pnpm check` | Type-check with svelte-check |
| `pnpm bindings` | Regenerate the TypeScript bindings from Rust |
| `cargo test` | Backend tests |

## Project layout

```
crates/core/   Backup, restore and ADB logic (Rust)
src-tauri/     Tauri application and commands
src/           UI (SvelteKit + Svelte 5 + TypeScript)
helper/        Helper that runs on the device
scripts/       platform-tools download and helper build
docs/          Legal notes on redistributing adb
```

## Built with

[Tauri 2](https://tauri.app/) · [SvelteKit](https://svelte.dev/docs/kit) · Rust · TypeScript

## Releases

Pushing a `v*` tag makes the [`release.yml`](.github/workflows/release.yml) workflow build the installers for all three platforms and attach them to a draft GitHub release.

## Licences

Code released under the [GNU General Public License v3.0 or later](LICENSE) (`GPL-3.0-or-later`).

The installers include Google's `adb` (platform-tools, Apache 2.0). See [`docs/platform-tools-licence.md`](docs/platform-tools-licence.md) for the analysis of redistributing it.

## Disclaimer

Working with root access and copying/restoring system data can leave a device unstable if something goes wrong. Use this tool at your own risk and verify your backups before deleting anything.
