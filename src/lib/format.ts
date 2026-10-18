import { get } from "svelte/store";
import { locale } from "svelte-i18n";

const UNITS = ["B", "KB", "MB", "GB", "TB"];

/** "12,3 GB" / "12.3 GB" depending on the active language (ui-spec §7). */
export function formatSize(bytes: number, lang = get(locale) ?? "en"): string {
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit++;
  }
  const digits = unit === 0 || value >= 100 ? 0 : 1;
  const n = new Intl.NumberFormat(lang, { maximumFractionDigits: digits, minimumFractionDigits: digits });
  return `${n.format(value)} ${UNITS[unit]}`;
}

export function formatTime(ms: number, lang = get(locale) ?? "en"): string {
  return new Intl.DateTimeFormat(lang, { timeStyle: "medium" }).format(ms);
}

/** Date and time of an RFC 3339 timestamp, in the active language. */
export function formatDate(iso: string, lang = get(locale) ?? "en"): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return new Intl.DateTimeFormat(lang, { dateStyle: "medium", timeStyle: "short" }).format(d);
}

/** Date and time of a millisecond timestamp, in the active language. */
export function formatMs(ms: number, lang = get(locale) ?? "en"): string {
  return new Intl.DateTimeFormat(lang, { dateStyle: "medium", timeStyle: "short" }).format(ms);
}

/** "1:05" / "1:02:03" for a duration in seconds. */
export function formatDuration(seconds: number): string {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = String(seconds % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}
