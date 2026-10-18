<script lang="ts">
  import { iconSrc } from "$lib/backup.svelte";

  let { app, size = 28 }: { app: { package: string; icon?: string | null }; size?: number } = $props();
  const src = $derived(iconSrc(app));
</script>

{#if src}
  <img {src} alt="" width={size} height={size} />
{:else}
  <span class="placeholder" style:width="{size}px" style:height="{size}px" aria-hidden="true"></span>
{/if}

<style>
  img,
  .placeholder {
    flex: none;
    border-radius: 22%;
  }
  .placeholder {
    display: inline-block;
    background: var(--surface-2);
    border: 1px solid var(--border);
  }
</style>
