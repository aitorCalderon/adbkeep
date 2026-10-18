import {
  commands,
  type CoreError,
  type DataPart,
  type OtherItem,
  type OtherRestoreSelection,
  type PreviewApp,
  type RestorePreview,
  type RestoreRequest,
} from "./bindings";
import { settings } from "./settings.svelte";
import { beginJob, clearJob, job, onJobFinished } from "./job.svelte";
import { permissionKeys } from "./backup.svelte";

export interface RestoreChoice {
  app: boolean;
  data: DataPart[];
  permissions: boolean;
  excluded: string[];
}

export type RestoreTab = "user" | "system" | "other" | "warnings";

/** A single app or Resto category chosen from Mis copias ("Restaurar…"). */
export type RestoreOnly = { kind: "app"; package: string } | { kind: "other"; item: OtherItem };

export const restore = $state({
  backupPath: null as string | null,
  preview: null as RestorePreview | null,
  loading: false,
  error: null as CoreError | null,
  choices: {} as Record<string, RestoreChoice>,
  other: emptyOther(),
  installAsPlay: false,
  bypassLowTargetSdk: false,
  tab: "user" as RestoreTab,
  simulation: null as string[] | null,
  /** Everything else is unticked after loading; kept while the same backup is reloaded. */
  only: null as RestoreOnly | null,
  /** "Antes de empezar" ticks; reminders only. */
  checklist: {} as Record<string, boolean>,
});

onJobFinished(() => {
  if (job.kind === "restore") restore.tab = "warnings";
});

function emptyOther(): OtherRestoreSelection {
  return {
    sms: false, contacts: false, contactsAsVcf: false, calls: false, blocked: false, calendar: false, dictionary: false,
    wifi: false, settings: [], sounds: false, wallpaper: false, certificates: false, folders: [],
  };
}

/** Everything restorable is ticked (ui-spec §4.2); the rest stays off. */
function defaultOther(p: RestorePreview): OtherRestoreSelection {
  const ok = (item: string) => p.other.some((o) => o.item === item && o.status !== "blocked");
  return {
    sms: ok("sms"),
    contacts: ok("contacts"),
    contactsAsVcf: false,
    calls: ok("calls"),
    blocked: ok("blocked"),
    calendar: ok("calendar"),
    dictionary: ok("dictionary"),
    wifi: ok("wifi"),
    settings: ok("settings") ? p.savedSettings : [],
    sounds: ok("sounds"),
    wallpaper: ok("wallpaper"),
    certificates: ok("certificates"),
    folders: ok("files") ? p.savedFolders : [],
  };
}

export function otherRestoreCount(o: OtherRestoreSelection): number {
  return [o.sms, o.contacts, o.calls, o.blocked, o.calendar, o.dictionary, o.wifi, o.settings.length > 0, o.sounds, o.wallpaper, o.certificates]
    .filter(Boolean).length + o.folders.length;
}

function defaultChoice(a: PreviewApp, entryData: DataPart[], hasPerms: boolean): RestoreChoice {
  const blocked = a.compat.status === "blocked";
  return {
    app: !blocked,
    data: !blocked && a.compat.dataAllowed ? entryData : [],
    permissions: !blocked && hasPerms,
    excluded: [],
  };
}

/** Loads the backup's apps and their compatibility with the connected phone. */
export async function loadPreview(
  path: string | null = restore.backupPath,
  only: RestoreOnly | null = path === restore.backupPath ? restore.only : null,
) {
  restore.backupPath = path;
  restore.only = only;
  restore.preview = null;
  restore.error = null;
  restore.choices = {};
  restore.other = emptyOther();
  if (!path) return;
  restore.loading = true;
  const res = await commands.restorePreview(path, { bypassLowTargetSdk: restore.bypassLowTargetSdk }, settings.useHelper);
  restore.loading = false;
  if (restore.backupPath !== path) return;
  if (res.status === "error") {
    restore.error = res.error;
    return;
  }
  restore.preview = res.data;
  restore.other = defaultOther(res.data);
  for (const a of res.data.apps) {
    const entry = res.data.manifest.apps.find((e) => e.package === a.meta.package);
    const parts = entry?.data.map((d) => d.part) ?? [];
    const perms = !!entry?.permissions && permissionKeys(a.meta.permissions).length > 0;
    restore.choices[a.meta.package] = defaultChoice(a, parts, perms);
  }
  if (only) applyOnly(res.data, only);
}

/** Resto selection fields of each item. */
const OTHER_FIELDS: Partial<Record<OtherItem, (keyof OtherRestoreSelection)[]>> = {
  sms: ["sms"], contacts: ["contacts"], calls: ["calls"], blocked: ["blocked"], calendar: ["calendar"],
  dictionary: ["dictionary"], wifi: ["wifi"], settings: ["settings"], sounds: ["sounds"], wallpaper: ["wallpaper"],
  certificates: ["certificates"], files: ["folders"],
};

function applyOnly(p: RestorePreview, only: RestoreOnly) {
  const defaults = restore.other;
  restore.other = emptyOther();
  for (const [pkg, c] of Object.entries(restore.choices)) {
    if (only.kind === "app" && pkg === only.package) continue;
    c.app = false;
    c.data = [];
    c.permissions = false;
  }
  if (only.kind === "app") {
    const kind = p.apps.find((a) => a.meta.package === only.package)?.meta.kind;
    restore.tab = kind === "updatedSystem" ? "system" : "user";
  } else {
    const other = restore.other as Record<string, unknown>;
    for (const f of OTHER_FIELDS[only.item] ?? []) other[f] = (defaults as Record<string, unknown>)[f];
    restore.tab = "other";
  }
}

export function entryParts(pkg: string): DataPart[] {
  return restore.preview?.manifest.apps.find((e) => e.package === pkg)?.data.map((d) => d.part) ?? [];
}

export function buildRestoreRequest(): RestoreRequest {
  return {
    backupPath: restore.backupPath ?? "",
    apps: (restore.preview?.apps ?? [])
      .filter((a) => {
        const c = restore.choices[a.meta.package];
        return c && (c.app || c.data.length > 0 || c.permissions);
      })
      .map((a) => {
        const c = restore.choices[a.meta.package];
        return {
          package: a.meta.package,
          app: c.app,
          data: c.data,
          permissions: c.permissions,
          excludedPermissions: c.permissions ? c.excluded : [],
        };
      }),
    installAsPlay: restore.installAsPlay,
    bypassLowTargetSdk: restore.bypassLowTargetSdk,
    other: restore.other,
  };
}

export async function startRestore(): Promise<CoreError | null> {
  beginJob("restore");
  const res = await commands.startRestore(buildRestoreRequest(), settings.useHelper);
  if (res.status === "error") {
    clearJob();
    job.running = false;
    return res.error;
  }
  return null;
}

export async function simulateRestore(): Promise<CoreError | null> {
  const res = await commands.simulateRestore(buildRestoreRequest());
  if (res.status === "error") return res.error;
  restore.simulation = res.data;
  return null;
}

export async function resetRestore() {
  clearJob();
  restore.only = null;
  restore.tab = "user";
  await loadPreview();
}
