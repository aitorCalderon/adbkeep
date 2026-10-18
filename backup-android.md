# Android backup via ADB root → restore on another device

## Context
You have root access over ADB: `su` on a rooted phone (Magisk, KernelSU…) or `adb root` (userdebug/eng
builds, and ROMs such as LineageOS with *Developer options → Rooted debugging*; after `adb root`, adbd
restarts and the phone asks again to authorize the PC).
You want to know **what** can be backed up and **how** each part works, so that the backup can be
restored on **another phone**, possibly running a **different Android version**.
This document only explains what to back up and how it works; the implementation will be done in a later session.

### General principle (key to portability)
There are two ways to back up each thing:
1. **Raw file copy** (`tar` of `/data/...`): faithful, but tied to UIDs, SELinux contexts, internal
   formats (databases, binary ABX XML on Android 12+) and hardware keys.
2. **Logical export via API/commands** (`content query`, `settings list`, `pm`, `cmd wifi`, `cmd role`…):
   less faithful, but **portable across devices and versions**, because on restore the target system's
   API is used (`content insert`, `settings put`, `pm grant`…).

Rule: **app data → raw copy + UID/SELinux fix-up; system data → logical export.**

Safe binary transport — **always end the pipeline with `| cat`**:
```
adb exec-out "su -c 'tar -cpf - -C /data/data com.example' | cat" > com.example.data.tar   # su
adb exec-out "tar -cpf - -C /data/data com.example | cat" > com.example.data.tar           # adb root
```
Why: if the process writes to a pseudo-terminal, the driver translates every `\n` into `\r\n` and the
tar arrives corrupted. Magisk's `su` allocates its own pty when invoked from `adb exec-out`
(verified: `tty` → `/debug_ramdisk/.magisk/pts/N`), so `adb exec-out "su -c 'tar …'"` **without**
`| cat` corrupts the output. With `| cat`, `tar`'s output goes to a pipe and arrives intact in every
case tested (Magisk and `adb root`). `adb shell "…" > file` also works when the local output is not a
terminal (adb uses the shell v2 protocol without a pty), but `exec-out … | cat` is the most robust.
Always check the result with `tar -tf` on the PC.

Two caveats verified on a Pixel 10 Pro (Android 16, Magisk 30700):
- **stderr gets mixed into the data**: both `exec-out` and Magisk's `su` pty put stderr in the same
  stream (`tar: had errors` showed up inside the tar). Redirect stderr inside the `su`:
  `su -c 'tar … 2>/dev/null'`.
- **Exit code**: `exec-out` always returns 0 and `| cat` hides `tar`'s. Use
  `adb shell -T "set -o pipefail; su -c '… 2>/dev/null' | cat" > file`: `-T` avoids the pty, the
  shell v2 protocol propagates the exit code and `pipefail` keeps `tar`'s. The resulting tar is
  identical to the `exec-out` one.

On-device tools: toybox `tar` exists on Android ≥ 9 (busybox on earlier versions);
`abx2xml`/`xml2abx`, `restorecon`, `content`, `cmd`, `appops`, `dpm` are present on all recent
versions. **`sqlite3` depends on the ROM**: LineageOS and other AOSP ROMs ship it, but vendor ROMs in
`user` builds (e.g. Samsung) do not. Check with `command -v sqlite3`.

Locate files instead of assuming paths: the provider's package and whether its DB lives in CE
(`/data/data`) or DE (`/data/user_de/0`) **varies between vendors and versions** (see items 4–6, 15).
Always search: `find /data/data /data/user_de/0 -maxdepth 3 -name mmssms.db` (likewise `contacts2.db`,
`calllog.db`, `blockednumbers.db`, `calendar.db`, `user_dict.db`). There may be **several copies**
(e.g. `mmssms.db` in both CE and DE): the valid one is the one with data; check both.

Binary XML (ABX): many files under `/data/system` are ABX on Android 12+ (they start with the bytes
`ABX\0`), but **not all** (e.g. `WifiConfigStore.xml` is still text XML). Check the header with
`head -c4 <f> | od -c` before converting. `abx2xml <f> -` can **fail midway** if any value contains
`U+0000` (seen in `notification_policy.xml`): it prints the part already converted and the error goes
to stderr, so the output ends up truncated without warning. Always check the exit code; if it fails,
copy the file to the PC and parse the ABX there with your own parser.

### Reliable extraction from content providers
Several items (contacts, calls, SMS/MMS, calendar, dictionary, blocked numbers) use `content query`.
Its output is `Row: N col=value, col=value, ...` **unescaped**: a value containing `, ` or `col=`, or
line breaks (SMS bodies, contact notes, event descriptions), makes the output ambiguous and impossible
to parse reliably. It is good for inspecting, not for extracting.
Reliable ways to extract:
(Verified with real data: of 187 SMS, 7 spanned several lines in the `content query` output and 18
contained `, `.)
1. **`sqlite3` on the device** reading the provider's DB as root, only to *extract* (the DB is not
   copied to the target): `sqlite3 -readonly -json <db> "select …"`. Only if the ROM ships `sqlite3`;
   otherwise a static binary for the phone's ABI would have to be uploaded. Downside: you need to know
   the schema, which varies between versions and vendors.
2. **Copy the DB to the PC and query it there** with `sqlite3` or any SQLite library: requires nothing
   on the phone. Copy the `.db` together with `-wal`/`-shm` if they exist (otherwise the latest writes
   are lost) and, ideally, with the provider stopped or at a quiet moment. Same schema downside.
3. **Own helper run on the device** (a `.dex` launched with `app_process`, or an auxiliary APK) that
   uses `ContentResolver` and writes JSON. It uses the public API (columns stable across versions) and
   escapes values properly; it is the most portable option.

---

## List of items and how each one works

### 1. APKs of installed apps  — portable ✅
- **Source**: `pm list packages -3` (user apps) and `pm path <pkg>` → `/data/app/~~xxx/<pkg>-yyy/base.apk` + `split_*.apk` (App Bundles: language, density, ABI).
- **Backup**: `adb pull` of *all* APKs (base + splits). Also store `versionCode`, `minSdk`, installer (`dumpsys package <pkg>`).
- **Restore**: `adb install-multiple -r base.apk split_*.apk` (optionally `-i com.android.vending` so it shows as installed from Play).
- **Risks across versions/phones**:
  - `minSdkVersion` > target Android → won't install.
  - Android 14+ blocks apps with `targetSdk < 23` (can be forced with `--bypass-low-target-sdk-block`; the option does not appear in `pm install` help, not verified in practice).
  - ABI splits (arm64 vs armeabi) or density different from the target phone → better to reinstall from Play and restore only data.
  - If the app already exists on the target with a different signature (e.g. a vendor system app) → signature conflict.

### 2. Private app data — portable with adjustments ⚠️
Locations per package (user 0):
| Path | What it is |
|---|---|
| `/data/data/<pkg>` (= `/data/user/0/<pkg>`) | CE storage (encrypted by credential): `databases/`, `shared_prefs/`, `files/`, `app_*/` |
| `/data/user_de/0/<pkg>` | DE storage (available before unlock): alarms, etc. |
| `/sdcard/Android/data/<pkg>` | App external data — owner `<uid>:ext_data_rw` (1078) |
| `/sdcard/Android/obb/<pkg>` | Expansions (games) — owner `<uid>:ext_obb_rw` (1079) |
| `/sdcard/Android/media/<pkg>` | App media (e.g. WhatsApp) — owner `<uid>:media_rw` (1023) or `<uid>:<uid>`, varies |

(In real storage, `/sdcard` = `/data/media/0`; with root always read and write `/data/media/0`. The groups in the table were seen identical on Samsung/Android 15 and LineageOS/Android 16; on restore they must be reproduced with the new UID, `<uid>:<uid>` is not enough.)

- **Backup**: `am force-stop <pkg>` and then `tar`, excluding `cache/`, `code_cache/`, `lib` (symlink to native libs) and normally `no_backup/` (the app marks it as non-portable: FCM tokens, install IDs).
- **Restore** (order matters):
  1. Install the APK first (creates the directory and assigns the UID).
  2. `am force-stop <pkg>`.
  3. Read the **new UID**: `stat -c %u /data/data/<pkg>` (the UID almost never matches between phones: `u0_a123` vs `u0_a245`).
  4. Extract the tar over the directory.
  5. `chown -R <uid>:<uid> /data/data/<pkg>` (and for `cache`/`code_cache` the group is `<uid>` + cache gid `20000+appid`; recreating them empty is enough).
  6. `restorecon -RF /data/data/<pkg>` → reassigns the correct SELinux context **of the target** (`u:object_r:app_data_file:s0:c123,c256,…`); the MLS categories depend on the UID, which is why the source's xattrs must not be copied.
  7. Same for `user_de` and the `/sdcard/Android/...` paths.
- **What does NOT survive** (physical limitation, not of the method):
  - **Android Keystore / KeyMint** keys (in TEE/StrongBox). Apps that encrypt their DB with them (Signal, banks, password managers, some 2FA) become unusable → use the app's own backup.
  - **FCM/push** tokens and sessions tied to device ID / Play Integrity → the app usually asks to log in again.
  - **Version downgrade**: data from a newer app version on an older version tends to fail (DB schema). Install the same or a higher app version.
- **SSAID (per-app ANDROID_ID)**: since Android 8 each app sees a different ANDROID_ID, stored in `/data/system/users/0/settings_ssaid.xml` (ABX on Android 12+; `abx2xml` converts it fine). Each entry is keyed **by UID** (`<setting name="10245" value="…" package="com.whatsapp" …/>`), so migrating it requires translating to the target's new UID. Some apps detect the change and log out — migrating it is advanced and optional.

### 3. Runtime permissions, AppOps and special access — portable via commands ✅
- **Raw source**: Android 11+: `/data/misc_de/0/apexdata/com.android.permission/`. On recent versions (seen on Android 15 and 16) `runtime-permissions.xml` in that folder only lists `shared-user`, and granted permissions are in `access.abx` (ABX); `roles.xml` stores the roles. Do not copy raw: the format changes between versions.
- **Logical backup**: `dumpsys package <pkg>` → permissions with `granted=true`; `appops get <pkg>`.
- **Restore**: `pm grant <pkg> <permission>`, `appops set <pkg> <op> allow`.
- **Special access** (in `settings secure`): `enabled_notification_listeners`, `enabled_accessibility_services`, `default_input_method`/`enabled_input_methods`; battery whitelist with `dumpsys deviceidle whitelist +<pkg>`; device administrators with `dpm`.
- **Default apps (roles, Android 10+)**: `cmd role get-role-holders android.app.role.SMS` / `cmd role add-role-holder …` (SMS, DIALER, BROWSER, HOME, ASSISTANT).

### 4. Contacts — portable ✅
- **Source**: `contacts2.db` (and `profile.db`) in the contacts provider package: `com.android.providers.contacts` on AOSP/LineageOS, `com.samsung.android.providers.contacts` on Samsung (locate with `find`). Do not copy the DB raw between versions/vendors (different schema and accounts).
- **Which contacts to export**: filter by `account_type` in `raw_contacts`. Many contacts are created by app sync adapters (`com.whatsapp`, `org.telegram.messenger`…) or account sync adapters (Google), which recreate them on their own; what needs exporting are device/SIM contacts and those of accounts that won't be set up again.
- **Backup**:
  - The provider only delivers the vCard (`content://com.android.contacts/contacts/as_vcard/<lookup>` or `as_multi_vcard/<lookup1>:<lookup2>…`) through `openAssetFile`. **`content read` doesn't work**: it calls `openFile` and fails with `FileNotFoundException: No files supported by provider` (verified). Getting vCards requires the own helper (`ContentResolver.openAssetFileDescriptor`).
  - Alternative without the helper: dump `raw_contacts` + `data` (with `sqlite3` or the helper, see *Reliable extraction*) and rebuild the contacts.
- **Restore**: push the `.vcf` and open it with the Contacts app (`am start -a android.intent.action.VIEW -d file:///sdcard/x.vcf -t text/x-vcard`), or `content insert` into `raw_contacts`/`data`.
- If they are synced with Google, signing in is enough; this is only needed for "device"/SIM contacts.

### 5. Call log — portable ✅
- **Source**: `calllog.db` in the contacts provider package (Android 7+; same package as item 4).
- **Backup**: `content query --uri content://call_log/calls` → CSV/JSON (number, date, duration, type, name…).
- **Restore**: `content insert --uri content://call_log/calls --bind number:s:... --bind date:l:... --bind type:i:...` (as root/system, it passes the permission check).

### 6. SMS / MMS — portable ✅
- **Source**: `mmssms.db` + `app_parts/` (MMS attachments) of the `com.android.providers.telephony` package, in `/data/data/…/databases/` (CE) **or** `/data/user_de/0/…/databases/` (DE): it may exist in both with only one holding the messages (on LineageOS 16 they were in CE with the DE copy empty; on Samsung 15 only the DE existed). Locate and check both. Raw is not portable (schema and thread IDs).
- **Backup**: `content://sms` (address, body, date, type, read…); MMS: `content://mms`, `content://mms/<id>/part`, `content://mms/<id>/addr`. Extract the rows with a reliable method (see *Reliable extraction from content providers*).
- **Binary of each MMS part** (images, audio, video): the `part` row gives `_id`, `ct` (MIME type), `name`/`cl` and `_data` (path in `app_parts/`). Text parts (`ct=text/plain`) carry their content in the `text` column and have no file. Two ways:
  - `adb exec-out "su -c 'content read --uri content://mms/part/<_id>'" > part_<_id>.bin` (opens the file via the provider).
  - Direct root: `tar` of `app_parts/` (in CE or DE, the same location as the `mmssms.db` with data) and match each file to its part by the `_data` column.
  - (Not verified with data: neither test phone had any MMS.)
- **Restore**: `content insert --uri content://sms ...` (threads are recreated on their own). App-compatible alternative: generate the *SMS Backup & Restore* XML.

### 7. Calendar (local) and alarms
- **Calendar**: `content://com.android.calendar/events` (+ `reminders`, `attendees`) → export and insert into a local calendar on the target. Google ones sync on their own.
- **Alarms**: they live in the clock app's data (Google Clock, Samsung clock…). If the target uses the same app → item 2; otherwise they must be recreated (`am start -a android.intent.action.SET_ALARM --ei android.intent.extra.alarm.HOUR …`).

### 8. Wi-Fi networks — portable ✅
- **Source**: `WifiConfigStore.xml` — Android 11+: `/data/misc/apexdata/com.android.wifi/` (next to `WifiConfigStoreSoftAp.xml`, the access-point config). It is **text XML, not ABX** (verified on Android 15 and 16). It contains SSID, security type and PSK in clear text (EAP/certificates separately).
- **Backup**: copy the XML.
- **Portable restore**: parse SSID/PSK and recreate with `cmd wifi add-network "<SSID>" open|owe|wpa2|wpa3|wep "<pass>"` (extra options `-h` hidden, `-r` MAC randomization…). Copying the XML between different versions is not reliable.

### 9. System settings — partially portable ⚠️
- **Source**: `/data/system/users/0/settings_{system,secure,global}.xml` (ABX on Android 12+). The number of keys varies a lot per ROM (≈ 330 per table on Samsung versus 60–150 on LineageOS), which confirms that most are vendor-specific.
- **Backup**: `settings list system|secure|global`.
- **Restore**: `settings put` **only from a whitelist** of generic keys (brightness/screen timeout, `font_scale`, rotation, ringtones, location mode, keyboard, accessibility, DND…). Many keys are vendor- or hardware-specific and can break things if copied blindly.

### 10. Ringtones, wallpaper, dictionary
- **Ringtones**: files in `/sdcard/{Ringtones,Notifications,Alarms}` + URIs in `settings system ringtone/notification_sound/alarm_alert`. The URI looks like `content://media/internal/audio/media/106?title=Orion&canonical=1`: the numeric ID changes on the target, but the `title` parameter allows re-resolving it by searching MediaStore for that title (after scanning the custom files).
- **Wallpaper**: in `/data/system/users/0/`: `wallpaper` (image cropped for the screen), `wallpaper_orig` (original image; JPEG or PNG) and `wallpaper_info.xml` (ABX). The lock screen only has its own file if it uses a different wallpaper (`wallpaper_lock`/`wallpaper_lock_orig` on AOSP; some vendors use their own directory, e.g. `wallpaper_lock_images/` on Samsung). Portable restore: extract `wallpaper_orig` and set it with an app / `SET_WALLPAPER` intent; copying the files + reboot often works but depends on the version.
- **User dictionary**: `content://user_dictionary/words` (query/insert). Gboard also keeps its own dictionary in its data (item 2).

### 11. Shared internal storage (/sdcard) — portable ✅
- **Source**: `/data/media/0` (= `/sdcard`): `DCIM`, `Pictures`, `Download`, `Documents`, `Music`, `Movies`, `Android/media/...`.
- **Backup**: `tar` from `/data/media/0` (preserves dates; better than `adb pull` for many files), excluding `Android/data` and `Android/obb` if they already go with the apps, and `.thumbnails`.
- **Owners**: files belong to the **MediaProvider UID** (`com.android.providers.media.module` / `com.google.android.providers.media.module`, query with `pm list packages -U`) or to the app that created them, with group `media_rw` (1023); the root `/data/media/0` is `media_rw:media_rw`.
- **Restore**: extract into `/data/media/0`, `chown -R <target MediaProvider uid>:media_rw` (or whatever owner the target has, see `stat`), `restorecon -RF` and force a MediaStore scan (reboot, or `content call --uri content://media/ --method scan_volume --arg external_primary` on Android 11+).

### 12. User CA certificates and VPN
- User CAs: `/data/misc/user/0/cacerts-added/` (the directory doesn't exist if none is installed) → copy + `restorecon` (portable).
- VPN/Wi-Fi EAP profiles with a private key in Keystore → **not portable**; re-import the .p12.

### 13. Launcher / home screen
- Launcher data (item 2). Only useful if the target uses the same launcher; with a different resolution/grid it may end up misplaced.

### 14. Profiles and additional users
- Work profile / secondary users: same paths with `<userId>` (10, 11…) instead of 0 (`pm list users`). Same method repeated per user. Some vendors use their own IDs for their profiles (e.g. Samsung's Secure Folder, ID 150 or similar); treat them like any other user.

### 15. Blocked numbers — portable ✅
- **Source**: `/data/user_de/0/com.android.providers.blockednumber/databases/blockednumbers.db` (DE; the same on both phones tested).
- `content://com.android.blockednumber/blocked` (extract with a reliable method) → `content insert` on the target.

### 16. Notification and Do Not Disturb settings — partially portable ⚠️
- **Source**: `/data/system/notification_policy.xml` (ABX on Android 12+). It contains, per package, the importance, whether notifications are allowed and the settings the user changed on each **channel** (importance, sound, vibration, bubbles, lock screen), and the **Do Not Disturb** configuration (`<zen>`: mode, call/message exceptions, automatic rules by schedule/event).
- **Backup**: copy the XML. Converting it with `abx2xml` can **fail midway** (seen on LineageOS: a channel name contained `U+0000`) → parse the ABX on the PC if it fails (see the document header). As a readable view: `dumpsys notification`. General switches in `settings secure`/`global` (`zen_mode`, `notification_badging`, `lock_screen_show_notifications`…) are already covered by item 9.
- **Portability**: channels are created by each app on startup; user changes can only be reapplied *after* installing and opening the app. The XML schema changes between versions, so copying it raw is not advisable; parse it and reapply the generic parts.

### 17. List of configured accounts — informational (not migrated)
- **Source**: `/data/system_ce/0/accounts_ce.db` / `accounts_de.db` (not portable, see the final table).
- **Backup**: `dumpsys account` → under `Accounts: N`, one `Account {name=..., type=...}` line per account (verified) (e.g. `com.google`, `com.whatsapp`, `com.microsoft.office.outlook`…). Only name + type are stored, **no tokens**.
- **Use**: a checklist of what to log in to again on the target, and to know which contacts/calendars will sync on their own (no need to export them).

### 18. Apps that use Android Keystore — informational (warning)
- **Purpose**: the data of these apps (item 2) becomes unusable on another device because their keys are in the TEE. Detecting them lets you warn before the backup and recommend the app's own backup.
- **Source**:
  - Android 12+ (Keystore2): `/data/misc/keystore/persistent.sqlite`, table `keyentry`; entries with `domain = 0` (APP) have the owning app's **UID** in `namespace` (`domain = 2` are system SELinux namespaces, ignore). Verified on Android 15 and 16: `select distinct namespace from keyentry where domain=0 and namespace>=10000` (with `sqlite3` on the phone, or by copying the file to the PC).
  - Android ≤ 11: files in `/data/misc/keystore/user_0/` named `<uid>_USRPKEY_<alias>`, `<uid>_USRCERT_…`, etc.
- **Mapping UID → package**: `pm list packages -U`.
- **Caveat**: using Keystore doesn't always mean the data is lost (some apps only store a token and ask to log in again); it is a risk signal, not a certainty.

---

## Separate section: updated system apps (included by default)
Items 1–2 cover user apps (`pm list packages -3`). This section adds system apps, but only those that **meet all** of the following:
- It is a system app and has been updated: `pm list packages -s` ∩ the `UPDATED_SYSTEM_APP` flag in `dumpsys package <pkg>`.
- The update's APK is in `/data/app/...` (`pm path` returns paths under `/data/app`, not `/system`/`/product`/`/vendor`). Those APKs are complete (with dex and libs), unlike the ones in `/system`.
- It is not in the fixed exclusion list: GMS (`com.google.android.gms`), GSF (`com.google.android.gsf`), Play Store (`com.android.vending`), WebView/Trichrome (`com.google.android.webview`, `com.google.android.trichromelibrary`), Android System Intelligence (`com.google.android.as`), Private Compute Services (`com.google.android.as.oss`), Carrier Services (`com.google.android.ims`), Google Messages (`com.google.android.apps.messaging`), internal Google services seen on a Pixel (eSIM `com.google.android.euicc`, app verifier `com.google.android.verifier`, `com.google.android.partnersetup`, AICore `com.google.android.aicore`, Settings suggestions `com.google.android.settings.intelligence` and `com.google.android.odad`), and any app with a system `sharedUserId` (`android.uid.system`, `.phone`, …). Note: GMS used the `sharedUserId` `com.google.uid.shared`, which is not `android.uid.*`, and on Android 16 it no longer has one → exclusion by name is necessary. Trichrome carries the version in its name (`com.google.android.trichromelibrary_699813532`): compare it by prefix.
- Verified detection: `dumpsys package packages` → in each block `pkgFlags=[ … UPDATED_SYSTEM_APP … ]` and `codePath=/data/app/…`.

**Backup** (per package): base APK + splits from `/data/app`, versionCode, signature (`dumpsys package` → `signatures`), granted permissions, and data as in item 2 (`/data/data`, `/data/user_de`, `/sdcard/Android/{data,obb,media}`).

**Restore**:
1. Add the Google account on the target first (Chrome and other Google apps log out/wipe if it is missing).
2. If the package **exists** on the target: check the signature matches; if its versionCode < source → `install-multiple -r` of the saved APK; if it is ≥ → don't install.
3. If it **doesn't exist** on the target: install as a user app. `signature|privileged` permissions are not granted, so it may partly fail (typical for vendor apps on another brand).
4. Never downgrade: if after step 2/3 the target versionCode < source, skip the data.
5. Restore data (force-stop → tar → chown to the new UID → restorecon), permissions with `pm grant`, and post actions: `ime set` for keyboards, open/restart for clocks (reschedule alarms).

Note: what counts as a "system" app depends on the ROM. Some vendors preinstall apps as user apps (on Samsung, Chrome and Clock show up in `pm list packages -3`), and then they go through items 1–2, not this section. On ROMs without Google, the list is usually short (on LineageOS: F-Droid, and Play Store/GMS if they were installed as system).

Typical worthwhile candidates: Gboard, Google Clock, Chrome (without passwords, which live in Play Services), offline Maps maps; vendor apps (Samsung Notes, Pixel Recorder) only between the same brand.

---

## What can NOT (or should not) be migrated to another device ❌
| Item | Reason |
|---|---|
| Accounts (`/data/system_ce/0/accounts_ce.db`, `accounts_de.db`) | Google tokens tied to the device/GMS; they get invalidated. Log in again. |
| Google Play Services / GSF data | Device IDs; you would break verification. |
| Bluetooth (`/data/misc/bluedroid/bt_config.conf`) | Link keys are bound to the source adapter's MAC. Re-pair. |
| Screen lock (`locksettings.db`, gatekeeper, fingerprints, face) | Bound to the TEE. |
| Keystore/KeyMint keys, Widevine DRM | Hardware-bound. |
| EFS/modem/persist partitions (IMEI, calibration) | Unique to the device; backup only for **the same** phone. |
| `dd` image of `userdata` | FBE encryption with device keys; only restorable on the same device. |


---

## Verification
Checked in read-only mode (nothing restored) on:
- Samsung Galaxy S23 (SM-S911B), One UI / Android 15, `user` build, root with Magisk (`su`).
- OnePlus (CPH2449), LineageOS 23.2 / Android 16, `user` build, `adb root` (rooted debugging).

**Extraction** was verified for all items, except what had no data on either phone (MMS attachments, calendar events, blocked numbers, user CAs, secondary users), where only the paths and URIs were checked to respond. The **restore** steps (`install`, `pm grant`, `settings put`, `content insert`, `chown`/`restorecon`…) have not been tested.
