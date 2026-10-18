import { commands, type AdbStatus } from "./bindings";
import { settings } from "./settings.svelte";

export const adb = $state({ status: null as AdbStatus | null });

/** Locates adb using the path from Preferences, falling back to bundled/PATH. */
export async function configureAdb() {
  const res = await commands.setAdbPath(settings.adbPath);
  if (res.status === "ok") adb.status = res.data;
}
