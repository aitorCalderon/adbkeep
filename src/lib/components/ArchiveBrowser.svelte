<script lang="ts">
  import { _ } from "svelte-i18n";
  import { commands, type ArchiveEntry } from "$lib/bindings";
  import { formatMs, formatSize } from "$lib/format";
  import { load, pickFolder } from "$lib/viewer";
  import { errorText } from "$lib/errors";

  /** A `.tar.zst` of the backup, browsed like a folder tree. */
  let { path, rel }: { path: string; rel: string } = $props();

  type Node = { name: string; path: string; dir: boolean; size: number; mtime: number; count: number };

  let entries = $state<ArchiveEntry[] | null>(null);
  let error = $state<string | null>(null);
  let cwd = $state("");
  let selected = $state<string[]>([]);
  let saving = $state(false);
  let status = $state<{ text: string; bad: boolean } | null>(null);

  $effect(() => {
    load(commands.listArchive(path, rel)).then((r) => {
      entries = r.data;
      error = r.error;
      // A saved folder is one top-level directory: start inside it.
      const tops = new Set((r.data ?? []).map((e) => e.path.split("/")[0]));
      if (tops.size === 1 && (r.data ?? []).some((e) => e.path.includes("/"))) cwd = [...tops][0];
    });
  });

  /** Folder path → its children. Folders without their own entry (find | tar) are implied. */
  const tree = $derived.by(() => {
    const children = new Map<string, Map<string, Node>>();
    const node = (p: string, dir: boolean): Node => {
      const i = p.lastIndexOf("/");
      const parent = i < 0 ? "" : p.slice(0, i);
      let siblings = children.get(parent);
      if (!siblings) children.set(parent, (siblings = new Map()));
      let n = siblings.get(p);
      if (!n) siblings.set(p, (n = { name: p.slice(i + 1), path: p, dir, size: 0, mtime: 0, count: 0 }));
      return n;
    };
    for (const e of entries ?? []) {
      const parts = e.path.split("/");
      for (let i = 1; i < parts.length; i++) {
        const d = node(parts.slice(0, i).join("/"), true);
        if (!e.dir) {
          d.size += e.size;
          d.count++;
        }
      }
      const n = node(e.path, e.dir);
      n.mtime = e.mtime;
      if (!e.dir) n.size = e.size;
    }
    return children;
  });

  const rows = $derived(
    [...(tree.get(cwd)?.values() ?? [])].sort((a, b) => Number(b.dir) - Number(a.dir) || a.name.localeCompare(b.name)),
  );
  const crumbs = $derived(cwd ? cwd.split("/") : []);

  function toggle(p: string) {
    selected = selected.includes(p) ? selected.filter((s) => s !== p) : [...selected, p];
  }

  async function saveSelected() {
    const dest = await pickFolder();
    if (!dest) return;
    saving = true;
    const r = await commands.extractFromArchive(path, rel, selected, dest);
    saving = false;
    status =
      r.status === "ok"
        ? { text: $_("library.view.files.saved", { values: { count: r.data, path: dest } }), bad: false }
        : { text: errorText(r.error), bad: true };
    if (r.status === "ok") selected = [];
  }
</script>

{#if error}
  <p class="error-text">{error}</p>
{:else if !entries}
  <p class="muted">{$_("library.view.loading")}</p>
{:else}
  <div class="view-toolbar">
    <nav class="crumbs">
      <button class="link" onclick={() => (cwd = "")}>/</button>
      {#each crumbs as c, i}
        <span class="muted">›</span>
        <button class="link" onclick={() => (cwd = crumbs.slice(0, i + 1).join("/"))}>{c}</button>
      {/each}
    </nav>
    <span class="muted">{$_("library.view.files.selected", { values: { count: selected.length } })}</span>
    <button disabled={selected.length === 0 || saving} onclick={saveSelected}>
      {saving ? $_("library.actions.working") : $_("library.view.files.saveSelected")}
    </button>
  </div>
  {#if status}<p class="view-status" class:bad={status.bad}>{status.text}</p>{/if}
  <div class="view-pane">
    <table class="data-table">
      <thead>
        <tr>
          <th></th>
          <th>{$_("library.view.files.name")}</th>
          <th class="num">{$_("library.view.files.size")}</th>
          <th>{$_("library.view.files.date")}</th>
        </tr>
      </thead>
      <tbody>
        {#if cwd}
          <tr>
            <td></td>
            <td colspan="3">
              <button class="link" onclick={() => (cwd = cwd.includes("/") ? cwd.slice(0, cwd.lastIndexOf("/")) : "")}>
                ↑ {$_("library.view.files.up")}
              </button>
            </td>
          </tr>
        {/if}
        {#each rows as n (n.path)}
          <tr>
            <td><input type="checkbox" checked={selected.includes(n.path)} onchange={() => toggle(n.path)} aria-label={n.name} /></td>
            <td>
              {#if n.dir}
                <button class="link" onclick={() => (cwd = n.path)}>📁 {n.name}</button>
                <span class="muted small">{$_("library.view.files.items", { values: { count: n.count } })}</span>
              {:else}
                {n.name}
              {/if}
            </td>
            <td class="num">{formatSize(n.size)}</td>
            <td class="muted">{n.mtime ? formatMs(n.mtime * 1000) : ""}</td>
          </tr>
        {:else}
          <tr><td></td><td colspan="3" class="muted">{$_("library.view.files.emptyFolder")}</td></tr>
        {/each}
      </tbody>
    </table>
  </div>
{/if}

<style>
  .crumbs {
    display: flex;
    flex-wrap: wrap;
    gap: 0.25rem;
    align-items: center;
    flex: 1 1 12rem;
  }
  .link {
    border: none;
    background: none;
    color: var(--accent);
    padding: 0;
    text-align: start;
    overflow-wrap: anywhere;
  }
  .small {
    font-size: 0.85em;
  }
</style>
