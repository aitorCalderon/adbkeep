<script lang="ts">
  import "../app.css";
  import { isLoading } from "svelte-i18n";
  import { initI18n } from "$lib/i18n";
  import { loadSettings, settings } from "$lib/settings.svelte";

  let { children } = $props();

  $effect(() => {
    document.documentElement.dataset.theme = settings.theme;
    document.documentElement.dataset.systemFont = String(settings.systemFont);
  });

  const ready = (async () => {
    await loadSettings();
    try {
      await initI18n(settings.language);
    } catch (e) {
      console.error("language could not be set; using English", e);
      await initI18n("en");
    }
  })();
</script>

{#await ready then}
  {#if !$isLoading}
    {@render children()}
  {/if}
{:catch error}
  <p style="padding: 1rem">adbkeep could not start: {String(error)}</p>
{/await}
