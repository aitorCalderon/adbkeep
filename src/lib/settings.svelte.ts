import { load, type Store } from "@tauri-apps/plugin-store";
import { LANGUAGES, type LanguagePref } from "./i18n";

export const THEMES = ["system", "ocean", "forest", "sunset", "lavender", "midnight", "graphite", "contrast"] as const;
export type Theme = (typeof THEMES)[number];

export interface Settings {
  advanced: boolean;
  language: LanguagePref;
  backupDir: string | null;
  /** Custom adb executable; null picks the bundled one, then PATH. */
  adbPath: string | null;
  /** Copy the helper dex to the phone for names, icons and providers. */
  useHelper: boolean;
  /** Follow the system UI font and size; off uses the built-in 14px stack. */
  systemFont: boolean;
  /** Colour theme id; "system" follows the OS light/dark. */
  theme: Theme;
}

const DEFAULTS: Settings = { advanced: false, language: "system", backupDir: null, adbPath: null, useHelper: true, systemFont: true, theme: "system" };

let store: Store | null = null;

/** Persistent preferences (ui-spec §1 ⚙). Mutate through `setSetting` so they are saved. */
export const settings: Settings = $state({ ...DEFAULTS });

/** Whether `value` is acceptable for `key`; the file on disk may be old, edited or corrupt. */
function valid<K extends keyof Settings>(key: K, value: unknown): value is Settings[K] {
  switch (key) {
    case "advanced":
    case "useHelper":
    case "systemFont":
      return typeof value === "boolean";
    case "language":
      return value === "system" || LANGUAGES.some((l) => l === value);
    case "theme":
      return THEMES.some((t) => t === value);
    case "backupDir":
    case "adbPath":
      return value === null || (typeof value === "string" && value !== "");
  }
}

function assign<K extends keyof Settings>(key: K, value: unknown) {
  if (valid(key, value)) settings[key] = value;
}

/** Loads the saved preferences; anything unreadable or invalid falls back to the defaults. */
export async function loadSettings() {
  try {
    store = await load("settings.json", { defaults: { ...DEFAULTS }, autoSave: 200 });
    for (const key of Object.keys(DEFAULTS) as (keyof Settings)[]) {
      assign(key, await store.get(key));
    }
  } catch (e) {
    console.error("settings could not be loaded; using defaults", e);
  }
}

export async function setSetting<K extends keyof Settings>(key: K, value: Settings[K]) {
  settings[key] = value;
  try {
    await store?.set(key, value);
  } catch (e) {
    console.error(`setting ${key} could not be saved`, e);
  }
}
