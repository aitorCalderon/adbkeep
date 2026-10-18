<script lang="ts">
  import { onMount } from "svelte";
  import { _ } from "svelte-i18n";
  import { open } from "@tauri-apps/plugin-dialog";
  import { library, refreshLibrary, startLibrary } from "$lib/library.svelte";
  import { formatSize, formatDate } from "$lib/format";
  import { errorText } from "$lib/errors";
  import BackupDetail from "./BackupDetail.svelte";

  onMount(startLibrary);


  async function chooseDir() {
    const dir = await open({ directory: true, defaultPath: library.dir || undefined });
    if (typeof dir === "string") {
      library.dir = dir;
      await refreshLibrary();
    }
  }

  const current = $derived(library.backups.find((b) => b.path === library.selected) ?? null);
</script>

<div class="library">
  <aside>
    <div class="folder">
      <span class="muted">{$_("library.folder")}:</span>
      <span class="mono path" title={library.dir}>{library.dir}</span>
      <button onclick={chooseDir}>{$_("library.change")}</button>
      <button onclick={refreshLibrary} title={$_("library.refresh")} aria-label={$_("library.refresh")}>⟳</button>
    </div>
    {#if library.error}
      <p class="error-text">{errorText(library.error)}</p>
    {/if}
    {#if library.loading && library.backups.length === 0}
      <p class="muted">{$_("library.loading")}</p>
    {:else if library.backups.length === 0}
      <p class="muted">{$_("library.empty")}</p>
    {/if}
    <ul>
      {#each library.backups as b (b.path)}
        {@const m = b.manifest}
        <li>
          <button class="item" class:active={b.path === library.selected} onclick={() => (library.selected = b.path)}>
            <span class="name">{b.name}</span>
            <span class="meta">
              {#if m}{m.source.model} · Android {m.source.androidRelease} · {formatDate(m.createdAt)} · {/if}{formatSize(b.size)}
            </span>
            <span class="badges">
              {#if b.status.kind !== "complete"}
                <span class="badge {b.status.kind}">{b.status.kind === "encrypted" ? "🔒 " : ""}{$_(`library.status.${b.status.kind}`)}</span>
              {/if}
              {#if b.packed}<span class="badge">{$_("library.packed")}</span>{/if}
              {#if b.encrypted && b.status.kind !== "encrypted"}<span class="badge">🔒 {$_("library.encrypted")}</span>{/if}
            </span>
          </button>
        </li>
      {/each}
    </ul>
  </aside>
  <section class="detail">
    {#if current}
      {#key current.path}
        <BackupDetail backup={current} />
      {/key}
    {:else}
      <p class="muted">{$_("library.select")}</p>
    {/if}
  </section>
</div>

<style>
  .library {
    display: grid;
    grid-template-columns: minmax(16rem, 22rem) 1fr;
    height: 100%;
    min-height: 0;
  }
  aside {
    border-inline-end: 1px solid var(--border);
    overflow: auto;
    padding: 0.75rem;
    background: var(--surface);
  }
  .folder {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    margin-block-end: 0.75rem;
  }
  .path {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: start;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .item {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.15rem;
    width: 100%;
    text-align: start;
    margin-block-end: 0.35rem;
    border-color: transparent;
    background: transparent;
  }
  .item.active {
    background: var(--surface-2);
    border-color: var(--border);
  }
  .name {
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .meta {
    font-size: 0.85em;
    color: var(--muted);
  }
  .badges {
    display: flex;
    gap: 0.3rem;
  }
  .badge {
    font-size: 0.75em;
    padding: 0 0.45em;
    border-radius: 1em;
    border: 1px solid var(--border);
    color: var(--muted);
  }
  .badge.incomplete,
  .badge.interrupted,
  .badge.unreadable {
    border-color: var(--warn);
    color: var(--warn);
  }
  .detail {
    overflow: auto;
    min-height: 0;
    padding: 0.75rem 1rem;
  }
</style>
