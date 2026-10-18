import { beginJob, clearJob, job, onJobFinished, startJobEvents } from "./job.svelte";
import {
  commands,
  events,
  type Analysis,
  type AppInfo,
  type BackupRequest,
  type CoreError,
  type DataPart,
  type OtherAnalysis,
  type OtherSelection,
  type Permissions,
  type SpecialAccess,
} from "./bindings";
import { device } from "./device.svelte";
import { settings } from "./settings.svelte";

export const DATA_PARTS: DataPart[] = ["ce", "de", "extData", "obb", "media"];

export interface AppChoice {
  app: boolean;
  /** Data locations to save; empty = no data. */
  data: DataPart[];
  permissions: boolean;
  /** Permission keys unticked in advanced mode (see `permissionKeys`). */
  excluded: string[];
}

export type Tab = "user" | "system" | "other" | "warnings";

export const backup = $state({
  analysis: null as Analysis | null,
  /** Serial of the phone the analysis belongs to. */
  analysisSerial: null as string | null,
  choices: {} as Record<string, AppChoice>,
  includeNoBackup: false,
  compression: 3,
  pack: false,
  name: "",
  destDir: "",
  tab: "user" as Tab,
  simulation: null as string[] | null,
  /** Resto tab; filled with defaults when the analysis has its counts. */
  other: null as OtherSelection | null,
  /** Excluded system apps the user chose to include anyway (advanced). */
  forced: {} as Record<string, boolean>,
  /** Encrypt the pack (only with `pack`); the password is never stored. */
  encrypt: false,
  password: "",
  passwordRepeat: "",
  /** Incomplete backup folder being continued, with its saved selection. */
  resume: null as { path: string; name: string; request: BackupRequest } | null,
});

let listening = false;

export async function startBackupEvents() {
  if (listening) return;
  listening = true;
  await events.analysisEvent.listen((e) => applyAnalysis(e.payload));
  await startJobEvents();
  onJobFinished(() => {
    if (job.kind === "backup") backup.tab = "warnings";
  });
}

function applyAnalysis(a: Analysis) {
  if (a.stage === "done") analysingSerial = null;
  const finished = a.stage === "done" && backup.analysis?.stage !== "done";
  backup.analysis = a;
  if (a.other && !backup.other) backup.other = defaultOther(a.other);
  backup.analysisSerial = device.snapshot?.selected ?? null;
  for (const app of a.apps) {
    if (!backup.choices[app.package]) backup.choices[app.package] = defaultChoice(app);
    else if (a.stage !== "packages" && backup.choices[app.package].data.length === 0 && !touched.has(app.package)) {
      // Sizes arrived: tick the data locations that exist.
      backup.choices[app.package].data = existingParts(app);
    }
  }
  if (finished) applyResume();
}

/** Packages whose data choice the user changed, so later analysis stages don't override it. */
const touched = new Set<string>();

export function touch(pkg: string) {
  touched.add(pkg);
}

function defaultChoice(app: AppInfo): AppChoice {
  const on = !app.exclusion;
  return { app: on, data: existingParts(app), permissions: on && hasPermissions(app), excluded: [] };
}

/** Everything with content is ticked (ui-spec §3: "todo lo portable está marcado por defecto"). */
export function defaultOther(a: OtherAnalysis): OtherSelection {
  const helper = settings.useHelper;
  return {
    sms: helper && ((a.sms ?? 0) + (a.mms ?? 0)) > 0,
    contacts: helper ? a.contacts.filter((c) => c.local).map((c) => c.accountType) : [],
    calls: helper && (a.calls ?? 0) > 0,
    blocked: helper && (a.blocked ?? 0) > 0,
    calendar: helper && a.calendars.some((c) => c.events > 0),
    wifi: (a.wifiNetworks ?? 0) > 0,
    settings: a.settings.filter((s) => isWhitelisted(s.table, s.key)).map((s) => `${s.table}/${s.key}`),
    notifications: a.notificationPolicy,
    // Empty Ringtones/Notifications/Alarms folders take a few KB.
    sounds: a.soundsSize > 64 * 1024,
    wallpaper: a.wallpaperFiles.length > 0,
    dictionary: helper && (a.dictionary ?? 0) > 0,
    certificates: a.certificates > 0,
    folders: a.folders.filter((f) => f.name && !f.name.startsWith(".") && f.name !== "Android" && f.size > 64 * 1024).map((f) => f.name),
    exclude: [".thumbnails"],
  };
}

/** Same list as SETTINGS_WHITELIST in Rust. */
const WHITELIST = new Set([
  "system/screen_brightness", "system/screen_brightness_mode", "system/screen_off_timeout", "system/font_scale",
  "system/accelerometer_rotation", "system/haptic_feedback_enabled", "system/sound_effects_enabled", "system/dtmf_tone",
  "system/vibrate_when_ringing", "system/time_12_24", "system/pointer_speed", "secure/location_mode", "secure/ui_night_mode",
  "secure/long_press_timeout", "secure/multi_press_timeout", "secure/accessibility_display_magnification_enabled",
  "secure/high_text_contrast_enabled", "secure/accessibility_display_inversion_enabled", "secure/accessibility_captioning_enabled",
  "secure/show_ime_with_hard_keyboard", "secure/spell_checker_enabled", "secure/screensaver_enabled",
  "secure/charging_sounds_enabled", "global/auto_time", "global/auto_time_zone", "global/window_animation_scale",
  "global/transition_animation_scale", "global/animator_duration_scale", "global/stay_on_while_plugged_in",
]);

export function isWhitelisted(table: string, key: string): boolean {
  return WHITELIST.has(`${table}/${key}`);
}

/** Number of Resto categories selected (Tonos/fondo/diccionario count as one, like the UI). */
export function otherCategories(o: OtherSelection | null): number {
  if (!o) return 0;
  const personal = o.sounds || o.wallpaper || o.dictionary;
  return [o.sms, o.contacts.length > 0, o.calls, o.blocked, o.calendar, o.wifi, o.settings !== null, o.notifications, personal, o.certificates]
    .filter(Boolean).length + o.folders.length;
}

/** Bytes the Resto selection adds (shared-storage folders and sounds; the rest is small). */
export function otherSize(o: OtherSelection | null, a: OtherAnalysis | null | undefined): number {
  if (!o || !a) return 0;
  return a.folders.filter((f) => o.folders.includes(f.name)).reduce((s, f) => s + f.size, 0) + (o.sounds ? a.soundsSize : 0);
}

export function existingParts(app: AppInfo): DataPart[] {
  return DATA_PARTS.filter((p) => app.data[p] != null);
}

/** Serial of the phone being analysed right now, so repeated device updates don't restart it. */
let analysingSerial: string | null = null;

/** Analyses the ready phone unless that phone was already analysed (or is being analysed). */
export async function ensureAnalysis(force = false) {
  const serial = device.snapshot?.selected ?? null;
  if (!force && serial && backup.analysisSerial === serial && backup.analysis) return;
  if (serial && analysingSerial === serial) return;
  if (!force) {
    const existing = await commands.analysisSnapshot();
    if (existing && existing.stage === "done") {
      applyAnalysis(existing);
      return;
    }
  }
  backup.analysis = null;
  backup.choices = {};
  backup.other = null;
  backup.forced = {};
  touched.clear();
  if (job.kind === "backup" && !job.running) clearJob();
  analysingSerial = serial;
  const res = await commands.startAnalysis(settings.useHelper);
  if (res.status === "error") analysingSerial = null;
}

export function analysing(): boolean {
  return backup.analysis?.stage !== "done";
}

export function partSize(app: AppInfo, part: DataPart): number {
  return app.data[part] ?? 0;
}

/** Name to show: the phone's label, else the package. */
export function appName(app: { package: string; label?: string | null }): string {
  return app.label || app.package;
}

export function iconSrc(app: { icon?: string | null }): string | null {
  return app.icon ? `data:image/png;base64,${app.icon}` : null;
}

export function apkSize(app: AppInfo): number {
  return app.apks.reduce((s, a) => s + a.size, 0);
}

/** Bytes this app adds to the backup with its current choice. */
export function chosenSize(app: AppInfo, c: AppChoice | undefined): number {
  if (!c || !c.app) return 0;
  return apkSize(app) + c.data.reduce((s, p) => s + partSize(app, p), 0);
}

export function specialKey(s: SpecialAccess): string {
  return `special:${JSON.stringify(s)}`;
}

/** Same keys as `permission_keys` in Rust. */
export function permissionKeys(p: Permissions): string[] {
  return [
    ...p.runtime.map((r) => `runtime:${r}`),
    ...p.appOps.map((o) => `op:${o.op}`),
    ...p.special.map(specialKey),
    ...p.roles.map((r) => `role:${r}`),
  ];
}

/** Excluded system apps are not selectable unless forced. */
export function selectable(app: AppInfo): boolean {
  return !app.exclusion || !!backup.forced[app.package];
}

export function hasPermissions(app: AppInfo): boolean {
  return permissionKeys(app.permissions).length > 0;
}

export function defaultName(model: string): string {
  const d = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  const date = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}_${pad(d.getHours())}${pad(d.getMinutes())}`;
  return `${model.replace(/[^\p{L}\p{N}._-]+/gu, "_")}_${date}`;
}

export async function initDestination(model: string) {
  backup.destDir = settings.backupDir ?? (await commands.defaultBackupDir()) ?? "";
  backup.name = defaultName(model);
}

export function buildRequest(): BackupRequest {
  const apps = (backup.analysis?.apps ?? [])
    .filter((a) => backup.choices[a.package]?.app && selectable(a))
    .map((a) => {
      const c = backup.choices[a.package];
      return {
        package: a.package,
        apk: true,
        data: c.data,
        permissions: c.permissions,
        excludedPermissions: c.permissions ? c.excluded : [],
      };
    });
  return {
    destDir: backup.destDir,
    name: backup.name,
    apps,
    includeNoBackup: backup.includeNoBackup,
    compressionLevel: backup.compression,
    pack: backup.pack,
    password: backup.pack && backup.encrypt ? backup.password : null,
    resume: backup.resume?.path ?? null,
    other: backup.other ?? {
      sms: false, contacts: [], calls: false, blocked: false, calendar: false, wifi: false, settings: null,
      notifications: false, sounds: false, wallpaper: false, dictionary: false, certificates: false, folders: [], exclude: [],
    },
  };
}

export async function startBackup(): Promise<CoreError | null> {
  beginJob("backup");
  const res = await commands.startBackup(buildRequest(), settings.useHelper);
  if (res.status === "error") {
    clearJob();
    job.running = false;
    return res.error;
  }
  return null;
}

export async function simulateBackup(): Promise<CoreError | null> {
  const res = await commands.simulateBackup(buildRequest());
  if (res.status === "error") return res.error;
  backup.simulation = res.data;
  return null;
}

/** Whether the password fields allow starting (or there is no encryption). */
export function passwordOk(): boolean {
  return !backup.pack || !backup.encrypt || (backup.password.length > 0 && backup.password === backup.passwordRepeat);
}

/**
 * Continues an incomplete backup (Mis copias → Reanudar): its saved selection is applied to
 * the Copia screen once the phone is analysed, and the job writes into the same folder.
 */
export async function prepareResume(path: string, name: string): Promise<CoreError | null> {
  const res = await commands.savedBackupRequest(path);
  if (res.status === "error") return res.error;
  backup.resume = { path, name, request: res.data };
  backup.tab = "user";
  if (job.kind === "backup" && !job.running) clearJob();
  applyResume();
  return null;
}

/** Applies the saved selection of the backup being resumed once the analysis is there. */
function applyResume() {
  const r = backup.resume?.request;
  if (!r || backup.analysis?.stage !== "done") return;
  for (const c of Object.values(backup.choices)) c.app = false;
  for (const sel of r.apps) {
    backup.choices[sel.package] = { app: true, data: sel.data, permissions: sel.permissions, excluded: sel.excludedPermissions ?? [] };
    touched.add(sel.package);
    backup.forced[sel.package] = true;
  }
  backup.other = r.other ?? null;
  backup.includeNoBackup = r.includeNoBackup;
  backup.compression = r.compressionLevel;
  backup.pack = r.pack;
  backup.encrypt = false;
}

export function cancelResume() {
  backup.resume = null;
  if (device.snapshot?.info) backup.name = defaultName(device.snapshot.info.model);
}

/** Back to the selection after a finished backup. */
export function resetResult() {
  clearJob();
  backup.resume = null;
  backup.password = "";
  backup.passwordRepeat = "";
  backup.tab = "user";
  if (device.snapshot?.info) backup.name = defaultName(device.snapshot.info.model);
}
