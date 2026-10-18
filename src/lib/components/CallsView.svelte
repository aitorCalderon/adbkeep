<script lang="ts">
  import { _ } from "svelte-i18n";
  import { commands, type Call } from "$lib/bindings";
  import { formatDuration, formatMs } from "$lib/format";
  import { fold, load } from "$lib/viewer";

  let { path }: { path: string } = $props();

  let calls = $state<Call[] | null>(null);
  let error = $state<string | null>(null);
  let query = $state("");

  $effect(() => {
    load(commands.backupCalls(path)).then((r) => {
      calls = r.data;
      error = r.error;
    });
  });

  const ICONS: Record<number, string> = { 1: "↙", 2: "↗", 3: "✕", 4: "☏", 5: "⊘", 6: "⊘" };
  const kind = (k: number) => (k >= 1 && k <= 6 ? $_(`library.view.calls.kinds.${k}`) : $_("library.view.calls.kinds.other"));
  const q = $derived(fold(query.trim()));
  const visible = $derived((calls ?? []).filter((c) => !q || fold(`${c.name ?? ""} ${c.number}`).includes(q)));
</script>

{#if error}
  <p class="error-text">{error}</p>
{:else if !calls}
  <p class="muted">{$_("library.view.loading")}</p>
{:else if calls.length === 0}
  <p class="muted">{$_("library.view.calls.empty")}</p>
{:else}
  <div class="view-toolbar">
    <input type="search" placeholder={$_("library.view.search")} bind:value={query} />
    <span class="muted">{visible.length} / {calls.length}</span>
  </div>
  <div class="view-pane">
    <table class="data-table">
      <thead>
        <tr>
          <th>{$_("library.view.calls.type")}</th>
          <th>{$_("library.view.calls.who")}</th>
          <th>{$_("library.view.calls.date")}</th>
          <th class="num">{$_("library.view.calls.duration")}</th>
        </tr>
      </thead>
      <tbody>
        {#each visible as c}
          <tr class:missed={c.kind === 3}>
            <td><span aria-hidden="true">{ICONS[c.kind] ?? "·"}</span> {kind(c.kind)}</td>
            <td>
              {c.name ?? c.number}
              {#if c.name}<div class="muted mono">{c.number}</div>{/if}
            </td>
            <td>{formatMs(c.date)}</td>
            <td class="num mono">{formatDuration(c.duration)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{/if}

<style>
  .missed td:first-child {
    color: var(--error);
  }
</style>
