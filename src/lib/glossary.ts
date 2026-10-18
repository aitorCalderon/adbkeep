import { derived } from "svelte/store";
import { _ } from "svelte-i18n";

export type Term = "root" | "keystore" | "appData" | "appFiles" | "usbDebugging";

/** Plain wording in simple mode, technical wording in advanced mode (ui-spec §1 glossary). */
export const term = derived(_, (t) => (key: Term, advanced: boolean) =>
  t(`glossary.${advanced ? "advanced" : "simple"}.${key}`),
);
