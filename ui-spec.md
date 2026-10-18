# adbkeep UI specification

## Context
This document defines **what appears in the interface** of adbkeep. It is a desktop app for the PC that, with the
phone connected over USB, backs up Android through ADB root (`su` or `adb root`) and restores to another phone,
possibly running a different Android version. It does not choose a technology or describe the implementation.
The *what* and the *how* of each element are in `backup-android.md`; here they are cited as "item N".

Design decisions:
- **No step-by-step wizard**. Backup and Restore are each a single screen with **tabs**
  (User apps · System apps · Everything else · Warnings) and a **fixed action bar** at the bottom.
- For each app you choose separately whether it is **backed up or restored** and whether **its data is included**. User
  apps and system apps go in separate tabs.
- **Two modes**:
  - **Simple** (default): plain language and safe defaults.
  - **Advanced**: a global, persistent switch that, on the same screens, adds technical details and fine-grained
    control. It does not change the flow or the layout; it only adds information and controls.

In each tab, whatever appears under **Advanced adds** is only visible when advanced mode is on.

---

## 1. Overall window structure

```
┌───────────────────────────────────────────────────────────────────────┐
│ [Backup] [Restore] [My backups]                Advanced mode [ ]  ⚙   │  top bar
├───────────────────────────────────────────────────────────────────────┤
│ 📱 Galaxy S23 · Android 15 · 🔋 82% · Root ✔ · 41 GB free           ▾ │  phone bar
├───────────────────────────────────────────────────────────────────────┤
│ [User apps] [System apps] [Everything else] [Warnings ③]              │  tabs
│                                                                       │
│   … tab content …                                                     │
│                                                                       │
├───────────────────────────────────────────────────────────────────────┤
│ 34 apps (28 with data) · 4 categories · 12.3 GB   📁 ~/Backups/…      │  action bar
│                                                         [ Back up ]   │
├───────────────────────────────────────────────────────────────────────┤
│ ▸ Log (advanced mode only)                                            │  log panel
└───────────────────────────────────────────────────────────────────────┘
```

- **Top bar**:
  - **Backup**, **Restore** and **My backups** sections.
  - **Advanced mode** switch.
  - **Preferences** (⚙): default backup folder, language and advanced mode.
- **Connected phone bar**, always visible: model, Android version, battery, root status and free space.
  Clicking it opens the connection panel (§2).
  - Advanced adds: adb serial, root method (`su` Magisk/KernelSU or `adb root`), SDK level, ABI, build type
    (`user`/`userdebug`) and whether `sqlite3` is available.
- **Log panel** (advanced mode only), collapsible at the bottom:
  - Every adb/shell command run, with its output, stderr and exit code.
  - It can be copied, filtered and saved.

### Principles
- Everything that is portable is ticked by default.
- What cannot be migrated (the ❌ table in `backup-android.md`) is **never an option**: it only appears as a warning.
- Simple mode has no jargon. UI text glossary:

| Technical term | Text in simple mode |
|---|---|
| root / `su` / `adb root` | administrator access |
| Android Keystore / KeyMint | apps with protection tied to the phone |
| CE/DE data, `/data/data` | app data |
| `Android/data`, `obb`, `media` | app files |
| SSAID | (hidden) |
| USB debugging / authorize adb | allow the computer to access the phone |
| provider, `content://…` | (hidden) |

---

## 2. Phone connection panel (shared)
While the phone is not ready, the content of Backup and Restore is replaced by this panel. It also opens when the
phone bar is clicked.

States, each with its own guide:

| State | What is shown |
|---|---|
| No phone | Illustrated guide: turn on Developer options and USB debugging, and connect the cable. |
| Pending authorization | "Accept the “Allow USB debugging?” prompt on the phone". |
| No root | What is needed: Magisk/KernelSU (`su`) or *Rooted debugging* on LineageOS and similar ROMs. |
| Root denied | "Grant access in the Magisk/KernelSU prompt on the phone" and Retry. |
| Reauthorize after `adb root` | `adb root` restarts adbd and the phone asks to authorize the PC again (Context). |
| Phone locked | "Unlock the phone": without unlocking, app data (CE) cannot be read. |
| Ready | Phone summary and a button to continue. |

- If several phones are connected, a selector.
- Advanced adds:
  - Choosing the root method when more than one is available.
  - A tool check: `tar`, `abx2xml`, `restorecon`, `content`, `cmd` and `sqlite3`, each with ✔/✘.
  - Users and profiles on the phone (`pm list users`, item 14).

---

## 3. Backup section
When the phone connects it is analysed: apps, sizes, accounts, Keystore and the location of the databases. Progress
is shown on the screen itself, and the tabs fill in as each part finishes.

### 3.1 User apps tab (items 1–2)
List of apps from `pm list packages -3`. For each row:

| Element | Behaviour |
|---|---|
| Icon and name | — |
| Size | APK + data. |
| **Back up app** checkbox | Ticked by default. |
| **Include data** switch | On by default. Only enabled if the app is ticked; without it only the APK is saved. |
| **Permissions** checkbox | Ticked by default and only enabled if the app is ticked (item 3). Saves what the app has been granted: permissions, special permissions (AppOps), special access (notification access, accessibility, keyboard, unrestricted battery, device administrator) and whether it is the default app for something. |
| Role label | If the app is the default for something, below the name: "Default browser", "Default SMS app", "Active keyboard"… |
| ⚠️ Warning | If the app uses Keystore (item 18). On hover: "Its data may not work on another phone; use the app's own backup". |

List bar:
- Search.
- Sort by name or size.
- "Only with warnings" filter.
- Select all or none, "Data for all" or "Data for none", and "Permissions for all" or "Permissions for none".
- Total selected.

**Advanced adds:**
- Columns: package name, version/versionCode, APK size, data size and number of splits.
- Next to the Permissions checkbox, the count ("9 permissions · 2 AppOps"). Clicking it opens the detail: `granted=true`
  permissions, AppOps, special access and roles, each with its own checkbox.
- "Include data" broken down into checkboxes for CE (`/data/data`), DE (`/data/user_de`), `Android/data`,
  `Android/obb` and `Android/media` (item 2).
- Global options for the tab:
  - Include `no_backup/`, off by default (item 2).
  - Migrate the per-app ID (SSAID), off by default (item 2).

### 3.2 System apps tab
Only **updated** system apps whose APK is in `/data/app` (the "updated system apps" section of
`backup-android.md`). They are ticked by default.
- Same list and controls as the user tab.
- The excluded ones (GMS, GSF, Play Store, WebView, Messages, apps with a system `sharedUserId`…) appear dimmed at
  the end, with the reason and no checkbox.
- Fixed note at the top: "These apps only restore well on a phone of the same brand or that already has them".

**Advanced adds:**
- Columns: flags (`UPDATED_SYSTEM_APP`), `codePath`, `sharedUserId` and signature.
- The technical reason for each exclusion (fixed list or `sharedUserId`) and the option to **force** an excluded app,
  with confirmation.

### 3.3 Everything else tab
Groups with a checkbox, the size and a one-line explanation per element:

| Group | Element | Item | Note in the UI |
|---|---|---|---|
| Communications | SMS/MMS messages | 6 | Number of messages and conversations. |
| | Phone/SIM contacts | 4 | "Google contacts sync by themselves when you sign in". |
| | Call log | 5 | Number of calls. |
| | Blocked numbers | 15 | — |
| Organisation | Local calendar | 7 | "Google calendars sync by themselves". |
| System | Wi-Fi networks | 8 | Number of networks. |
| | Basic settings | 9 | Brightness, display, sounds, accessibility, keyboard… |
| | Notifications and Do Not Disturb | 16 | — |
| | Ringtones, wallpaper and dictionary | 10 | — |
| | User certificates | 12 | Only if any is installed. |
| Files | One checkbox per folder: Photos (DCIM), Pictures, Downloads, Documents, Music, Movies… | 11 | Size per folder. |

**Advanced adds:**
- Contacts: checkboxes per `account_type` (device, SIM, Google, WhatsApp…), with those of sync adapters
  unticked (item 4).
- Settings: the full table of `settings` system/secure/global, with the allowlisted keys pre-ticked and
  editable (item 9).
- Files: a tree of `/data/media/0` with editable exclusions. By default `.thumbnails`, `Android/data` and
  `Android/obb` are excluded (item 11).
- Additional profiles and users: one block per user (item 14).
- Provider extraction method: sqlite3 on the phone, copy the database to the PC, or helper ("Reliable extraction").
- Provider package and detected CE/DE location for each database, indicating which copy has data (items 4–6 and 15).

### 3.4 Warnings tab (pre-backup report)
The tab shows a counter if there are warnings.
- **Apps with protection tied to the phone** (item 18): a list linking to the app's row and recommending to use
  its own backup.
- **Configured accounts** (item 17): which ones you will need to sign in to again on the target and what will sync by itself.
- **What cannot be backed up** (❌ table): accounts, Bluetooth, screen lock and fingerprints, DRM, IMEI/EFS,
  each with its reason in one line.
- Advanced adds:
  - Keystore UIDs and namespaces, with the matching package.
  - The type of each account.

### 3.5 Action bar
- Summary: "34 apps (28 with data) · 4 categories · 12.3 GB".
- Destination: backup folder and name (by default model + date), with the free space on the PC.
  Shown in red if it does not fit.
- Fixed warning if Wi-Fi is backed up: "The backup contains the Wi-Fi passwords unencrypted".
- **Back up** button.
- Advanced adds:
  - **Simulate only**: generates and shows the commands without running them.
  - Compression level.

### 3.6 During the backup
- The tabs are locked (read-only). Each row shows its status: pending, running, done or error.
- The action bar shows the overall progress, the current element, "Don't disconnect the phone" and **Cancel**.
- Notice: "Apps are closed while their data is copied".
- Advanced adds: current file, speed and the log panel open.

### 3.7 When it finishes
The Warnings tab becomes **Result**:
- Summary per category and a list of errors, each linked to its row.
- Buttons to open the folder and save the report.
- Advanced adds: integrity verification (`tar -tf` of each archive) and the full session log.

---

## 4. Restore section
- **Header**: on the left, the backup selector (source phone, Android, date and size); on the right, the connected
  target phone. Once both are present, compatibility is computed.
- **The same tabs** as in Backup, but only with what the backup contains and with the compatibility of each
  element:
  - ✅ Will be restored.
  - ⚠️ With limitations.
  - ⛔ Cannot be restored.

  Each status carries a "Why?" link.

### 4.1 User apps and System apps tabs
For each app:

| Element | Behaviour |
|---|---|
| **Restore app** checkbox | Ticked except for the ⛔ ones, which appear unticked and locked. |
| **Restore data** switch | Disabled if the backup has no data for the app, or if the app installed on the target is older than the one in the backup and cannot be updated: data from a newer version on an older app tends to fail. |
| **Permissions** checkbox | Disabled if the backup has no permissions for that app. If the app is the default for something and the target already has another app for that, a warning is shown: "It will become the default browser instead of Chrome". |
| Compatibility | ✅/⚠️/⛔ with its reason in plain text. |
| Planned action | "Will be installed", "Will be updated", "Already installed: data only" or "Skipped". |

Reasons (item 1 and the system apps section):
- ⛔ Needs a newer Android (`minSdk`).
- ⛔ App too old for Android 14+ (`targetSdk < 23`).
- ⛔ Made for another type of processor (ABI).
- ⛔ Already on this phone with another signature.
- ⚠️ System app from another brand: installed as a normal app, it may not fully work.
- ⚠️ Data that depends on the phone (Keystore).

Same list actions and filters as in Backup, plus a filter by compatibility status.

**Advanced adds:**
- Source versionCode versus target.
- Source signature versus target.
- `minSdk` versus the target SDK.
- ABI of the splits.
- `signature|privileged` permissions that will not be granted.
- Detail of the permissions, AppOps, special access and roles to reapply, each with its checkbox (item 3).
- Checkbox to install as if from Play (`-i com.android.vending`).
- Data breakdown by location (CE, DE, `Android/data`, `obb`, `media`).
- Force `--bypass-low-target-sdk-block`, marked "unverified".

### 4.2 Everything else tab
The same groups as in Backup, with what cannot be restored disabled and its reason. Example: "Settings: only the
basic settings common to every phone are applied".
- Advanced adds:
  - The `settings` keys that will be written (editable).
  - Wi-Fi restore method: `cmd wifi add-network` (Android 11+) or copying the XML.
  - Target user for each profile in the backup.

### 4.3 Warnings tab
- **Before starting** (checklist):
  - [ ] Google account signed in on the target (needed for system apps and Chrome).
  - [ ] Enough battery or charger connected.
  - [ ] Phone unlocked.
  - [ ] Don't use the phone while restoring.
- Compatibility warnings grouped by type, each linking to its row.

### 4.4 Action bar, progress and result
- Action bar: summary of the selection and a **Restore** button (with **Simulate only** in advanced mode).
- Progress works the same as in backup (§3.6).
- When it finishes, the Warnings tab becomes **Result and pending tasks**:
  - [ ] Sign in again to these accounts: … (item 17).
  - [ ] Open the contacts file in the Contacts app, if they were restored as `.vcf` (item 4).
  - [ ] Pair your Bluetooth devices again.
  - [ ] Set up the screen lock and fingerprints.
  - [ ] Restore these apps with their own backup: … (item 18).
  - [ ] Open each app once so its notification settings apply (item 16).
  - [ ] Restart the phone (file scanning and wallpaper).
  - Button to export the list as a checklist.
  - Advanced adds, per app:
    - New UID.
    - Result of `chown` and `restorecon`.
    - Permissions that failed to be granted.

---

## 5. My backups section
- **Left column**: list of backups (name, source phone, Android, date and size). Actions:
  - **Restore**: opens the Restore section with that backup chosen.
  - **Verify integrity**.
  - **Delete**, with confirmation.
- **Right**: detail of the chosen backup, in tabs:

| Tab | Content |
|---|---|
| Summary | Source phone, Android, date, size and which categories it contains. |
| User apps | Name, version and whether it has data. |
| System apps | Same. |
| Messages | Conversation view, with search. |
| Contacts | List with detail. |
| Calls | List. |
| Files | Folder browser. |
| Wi-Fi | Networks, with passwords hidden and a button to show them. |
| Warnings | The warnings saved when the backup was made. |

- Individual exports: contacts to `.vcf`, SMS to the *SMS Backup & Restore* XML (item 6) and individual files.
- Advanced adds these tabs:
  - **Backup files**: the tar, JSON and metadata files as they are, with a viewer for XML converted from ABX.
  - **Settings**: saved keys.
  - **Log**: the log of the backup session.

---

## 6. Errors and global states

| Situation | Message and action |
|---|---|
| Phone disconnected midway | "The phone was disconnected". Resume on reconnecting, or discard. |
| Authorization lost | The connection panel returns in the "Pending authorization" state. |
| Root denied | "Grant administrator access on the phone". Retry. |
| Not enough space (PC or phone) | How much is missing, and the option to change folder or deselect items. |
| Phone locked during the process | Pause with "Unlock the phone to continue". |
| Failure in one element | The rest continues, the row is marked ⚠ and it is reported in Result. |

In advanced mode, each error links to its command in the log panel, with stderr and exit code. For example,
a truncated `abx2xml` (Context, binary XML).

---

## 7. Languages
- The interface is translatable. Initial languages: **Spanish** and **English**.
- **Selection**:
  - On first launch the PC's system language is used if available; otherwise **English** (the fallback language, also for strings missing from another translation).
  - It is changed in Preferences (⚙) and applies immediately, without restarting.
- **What is translated**:
  - All UI text.
  - The connection panel guides.
  - The compatibility reasons.
  - The warnings and the pending tasks checklist.
  - The simple-mode glossary (§1): each language has its own table of terms.
- **What is not translated**:
  - Commands and the log panel.
  - Package names, paths, `settings` keys, permissions and flags.
  - App names and user data (messages, contacts, files).
- **Language-dependent formats**: dates and times, the decimal and thousands separators in sizes ("12,3 GB" /
  "12.3 GB") and plurals ("1 app" / "34 apps").
- **Exported reports** (result, checklist): in the language active at export time. The backup's internal metadata
  does not depend on the language, so a backup made with the UI in one language opens the same in another.
- **Layout**:
  - Texts must not have a fixed width, because translations can be 30–40 % longer.
  - Guide images must not have embedded text.
  - If right-to-left languages (Arabic, Hebrew) are added in the future, the window must be able to mirror.

---

## 8. Decisions (formerly open questions)
Decided on 2026-10-15:
- **Encryption**: optional and only when packing into a single file, with a password. The backup folder stays
  in the clear. The encrypted package is a standard *age* file (`age -d backup.adbkeep | zstd -d | tar -x`). adbkeep does not store
  the password: it asks for it once per session to view, verify, unpack or restore the backup.
- **Incremental backups**: not for now. Each backup is complete and independent.
- **Restoring individual elements**: yes, per app and per category in Everything else. **Restore…** in My backups opens Restore
  with only that element ticked, and more can be ticked. Individual conversations and contacts are not restored.
- **Resume**: a backup that was cancelled, cut off or hit a full disk is left *Incomplete* and can be **Resumed** from My
  backups with the source phone connected. Only what was missing is copied. If the phone disconnects or restarts
  midway, the backup or restore pauses ("The phone was disconnected" / "Unlock the phone to continue") and
  continues by itself when it returns. **Discard** ends it as incomplete.
- Pending: which languages to add after Spanish and English.
