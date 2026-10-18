import { open, save } from "@tauri-apps/plugin-dialog";
import { errorText } from "./errors";
import type { CoreError } from "./bindings";

type Res<T> = { status: "ok"; data: T } | { status: "error"; error: CoreError };

/** Asks where to save one file; `null` if cancelled. */
export async function pickFile(defaultName: string, ext: string): Promise<string | null> {
  return save({ defaultPath: defaultName, filters: [{ name: ext.toUpperCase(), extensions: [ext] }] });
}

/** Asks for a folder; `null` if cancelled. */
export async function pickFolder(): Promise<string | null> {
  const r = await open({ directory: true });
  return typeof r === "string" ? r : null;
}

/** A command's data, or its error as text. */
export async function load<T>(p: Promise<Res<T>>): Promise<{ data: T | null; error: string | null }> {
  const r = await p;
  return r.status === "ok" ? { data: r.data, error: null } : { data: null, error: errorText(r.error) };
}

/** Case- and accent-insensitive search text. */
export function fold(s: string): string {
  return s.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
}
