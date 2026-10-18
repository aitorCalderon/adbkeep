<script lang="ts" module>
  export type Section = "backup" | "restore" | "library";
</script>

<script lang="ts">
  import { _ } from "svelte-i18n";
  import { settings, setSetting } from "$lib/settings.svelte";

  let { section = $bindable(), onPreferences }: { section: Section; onPreferences: () => void } = $props();

  const SECTIONS: Section[] = ["backup", "restore", "library"];
</script>

<header>
  <nav>
    {#each SECTIONS as s}
      <button class:active={section === s} aria-current={section === s ? "page" : undefined} onclick={() => (section = s)}>
        {$_(`nav.${s}`)}
      </button>
    {/each}
  </nav>
  <label class="advanced">
    {$_("topbar.advanced")}
    <input type="checkbox" checked={settings.advanced} onchange={(e) => setSetting("advanced", e.currentTarget.checked)} />
  </label>
  <button class="icon" title={$_("topbar.preferences")} aria-label={$_("topbar.preferences")} onclick={onPreferences}>⚙</button>
</header>

<style>
  header {
    display: flex;
    align-items: center;
    gap: 1rem;
    padding: 0.5rem 1rem;
    background: var(--surface);
    border-block-end: 1px solid var(--border);
  }
  nav {
    display: flex;
    gap: 0.25rem;
    flex: 1;
  }
  nav button {
    border-color: transparent;
    background: transparent;
  }
  nav button.active {
    background: var(--surface-2);
    border-color: var(--border);
    font-weight: 600;
  }
  .advanced {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }
  .icon {
    font-size: 1.1rem;
    padding: 0.1em 0.5em;
  }
</style>
