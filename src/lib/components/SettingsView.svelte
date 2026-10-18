<script lang="ts">
  import { _ } from "svelte-i18n";
  import { commands, type SettingView } from "$lib/bindings";
  import { fold, load } from "$lib/viewer";

  let { path }: { path: string } = $props();

  let entries = $state<SettingView[] | null>(null);
  let error = $state<string | null>(null);
  let query = $state("");
  let onlySelected = $state(false);

  $effect(() => {
    load(commands.backupSettings(path)).then((r) => {
      entries = r.data;
      error = r.error;
    });
  });

  const q = $derived(fold(query.trim()));
  const visible = $derived(
    (entries ?? []).filter((e) => (!onlySelected || e.selected) && (!q || fold(`${e.table}/${e.key} ${e.value}`).includes(q))),
  );
</script>

{#if error}
  <p class="error-text">{error}</p>
{:else if !entries}
  <p class="muted">{$_("library.view.loading")}</p>
{:else if entries.length === 0}
  <p class="muted">{$_("library.view.settings.empty")}</p>
{:else}
  <div class="view-toolbar">
    <input type="search" placeholder={$_("library.view.settings.filter")} bind:value={query} />
    <label><input type="checkbox" bind:checked={onlySelected} /> {$_("library.view.settings.onlySelected")}</label>
    <span class="muted">{visible.length} / {entries.length}</span>
  </div>
  <div class="view-pane">
    <table class="data-table">
      <thead>
        <tr>
          <th>{$_("library.view.settings.table")}</th>
          <th>{$_("library.view.settings.key")}</th>
          <th>{$_("library.view.settings.value")}</th>
          <th>{$_("library.view.settings.selected")}</th>
        </tr>
      </thead>
      <tbody>
        {#each visible as e}
          <tr>
            <td class="mono muted">{e.table}</td>
            <td class="mono">{e.key}</td>
            <td class="mono value">{e.value}</td>
            <td>{e.selected ? "✔" : ""}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{/if}

<style>
  .value {
    overflow-wrap: anywhere;
  }
</style>
