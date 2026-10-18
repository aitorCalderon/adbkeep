<script lang="ts">
  import { _ } from "svelte-i18n";
  import type { Manifest } from "$lib/bindings";
  import ArchiveBrowser from "./ArchiveBrowser.svelte";

  /** Shared storage folders saved in the Resto tab, one archive each. */
  let { path, manifest }: { path: string; manifest: Manifest } = $props();

  const archives = $derived(
    (manifest.other ?? [])
      .filter((e) => e.item === "files")
      .flatMap((e) => e.files)
      .filter((f) => f.startsWith("other/files/") && f.endsWith(".tar.zst")),
  );
  let current = $state<string | null>(null);
  const chosen = $derived(current ?? archives[0] ?? null);

  const label = (rel: string) => {
    const name = rel.slice("other/files/".length, -".tar.zst".length);
    return name === "_root" ? $_("library.view.files.root") : name;
  };
</script>

{#if archives.length === 0}
  <p class="muted">{$_("library.view.files.empty")}</p>
{:else}
  <div class="folders" role="tablist">
    {#each archives as a}
      <button role="tab" aria-selected={chosen === a} class:active={chosen === a} onclick={() => (current = a)}>📁 {label(a)}</button>
    {/each}
  </div>
  {#if chosen}
    {#key chosen}
      <ArchiveBrowser {path} rel={chosen} />
    {/key}
  {/if}
{/if}

<style>
  .folders {
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem;
    margin-block-end: 0.6rem;
  }
  .folders button.active {
    border-color: var(--accent);
    font-weight: 600;
  }
</style>
