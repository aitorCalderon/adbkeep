import { addMessages, init, locale } from "svelte-i18n";
import { locale as osLocale } from "@tauri-apps/plugin-os";
import en from "./en.json";
import es from "./es.json";

export const LANGUAGES = ["es", "en"] as const;
export type Language = (typeof LANGUAGES)[number];
/** "system" follows the computer's language (ui-spec §7). */
export type LanguagePref = Language | "system";

addMessages("en", en);
addMessages("es", es);

function supported(tag: string | null | undefined): Language | null {
  const base = tag?.toLowerCase().split(/[-_]/)[0];
  return LANGUAGES.find((l) => l === base) ?? null;
}

/** The computer's language if translated, else English. */
export async function systemLanguage(): Promise<Language> {
  try {
    return supported(await osLocale()) ?? supported(navigator.language) ?? "en";
  } catch {
    return supported(navigator.language) ?? "en";
  }
}

export async function initI18n(pref: LanguagePref) {
  init({ fallbackLocale: "en", initialLocale: await resolve(pref) });
}

export async function applyLanguage(pref: LanguagePref) {
  locale.set(await resolve(pref));
}

async function resolve(pref: LanguagePref): Promise<Language> {
  return pref === "system" ? systemLanguage() : pref;
}
