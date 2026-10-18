import { get } from "svelte/store";
import { _ } from "svelte-i18n";
import type { CoreError } from "./bindings";
import { formatSize } from "./format";

/** Translates an error coming from Rust (`code` + parameters). */
export function errorText(e: CoreError | unknown): string {
  const t = get(_);
  if (e && typeof e === "object" && "code" in e) {
    const { code, ...values } = e as CoreError & Record<string, unknown>;
    if (code === "notEnoughSpace") {
      values.needed = formatSize(values.needed as number);
      values.available = formatSize(values.available as number);
    }
    return t(`error.${code}`, { values: values as Record<string, string> });
  }
  return String(e);
}
