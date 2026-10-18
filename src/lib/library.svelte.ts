import { commands, events, type BackupListing, type CoreError, type VerifyReport } from "./bindings";
import { settings } from "./settings.svelte";

export const library = $state({
  dir: "",
  loading: false,
  backups: [] as BackupListing[],
  selected: null as string | null,
  error: null as CoreError | null,
  /** Path → running verification progress. */
  verifying: {} as Record<string, { done: number; total: number }>,
  reports: {} as Record<string, VerifyReport>,
  busy: null as string | null,
});

let listening = false;

export async function startLibrary() {
  if (!library.dir) library.dir = settings.backupDir ?? (await commands.defaultBackupDir()) ?? "";
  if (!listening) {
    listening = true;
    await events.verifyProgressEvent.listen((e) => {
      const { path, done, total } = e.payload;
      if (library.verifying[path]) library.verifying[path] = { done, total };
    });
  }
  await refreshLibrary();
}

export async function refreshLibrary() {
  if (!library.dir) return;
  library.loading = true;
  const res = await commands.listBackups(library.dir);
  library.loading = false;
  if (res.status === "error") {
    library.error = res.error;
    return;
  }
  library.error = null;
  library.backups = res.data;
  if (!library.backups.some((b) => b.path === library.selected)) library.selected = library.backups[0]?.path ?? null;
}

export async function verify(path: string) {
  library.verifying[path] = { done: 0, total: 0 };
  const res = await commands.verifyBackup(path);
  delete library.verifying[path];
  if (res.status === "ok") library.reports[path] = res.data;
  else library.error = res.error;
}

/** Runs a pack/unpack/delete and refreshes; selects `select(result)` afterwards. */
export async function runAction<T>(
  path: string,
  action: () => Promise<{ status: "ok"; data: T } | { status: "error"; error: CoreError }>,
  select?: (data: T) => string | null,
) {
  library.busy = path;
  const res = await action();
  library.busy = null;
  if (res.status === "error") {
    library.error = res.error;
    return;
  }
  delete library.reports[path];
  if (select) library.selected = select(res.data);
  await refreshLibrary();
}
