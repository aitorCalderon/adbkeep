import type { Section } from "./components/TopBar.svelte";

export const nav = $state({ section: "backup" as Section });
