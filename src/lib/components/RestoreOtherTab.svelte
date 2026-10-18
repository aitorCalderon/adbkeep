<script lang="ts">
  import { _ } from "svelte-i18n";
  import type { CompatStatus, ItemStatus, OtherCompat, OtherItem, OtherReason } from "$lib/bindings";
  import { restore } from "$lib/restore.svelte";
  import { settings } from "$lib/settings.svelte";

  let { items, locked, statuses }: { items: OtherCompat[]; locked: boolean; statuses: Record<string, ItemStatus> } = $props();

  const ICON: Record<CompatStatus, string> = { ok: "✅", limited: "⚠️", blocked: "⛔" };

  const o = $derived(restore.other);
  const folders = $derived(restore.preview?.savedFolders ?? []);

  function checked(item: OtherItem): boolean {
    switch (item) {
      case "settings":
        return o.settings.length > 0;
      case "files":
        return o.folders.length > 0;
      case "notifications":
        return false;
      default:
        return !!(o as unknown as Record<string, boolean>)[item];
    }
  }

  function set(item: OtherItem, on: boolean) {
    switch (item) {
      case "settings":
        o.settings = on ? (restore.preview?.savedSettings ?? []) : [];
        break;
      case "files":
        o.folders = on ? folders : [];
        break;
      case "notifications":
        break;
      default:
        (o as unknown as Record<string, boolean>)[item] = on;
    }
  }

  function reasonText(r: OtherReason): string {
    return r.kind === "wifiSkipped"
      ? $_("restore.other.reason.wifiSkipped", { values: { count: r.count } })
      : $_(`restore.other.reason.${r.kind}`);
  }

  function title(item: OtherItem) {
    return $_(`backup.other.items.${item}`);
  }

  function count(c: OtherCompat): string {
    if (c.item === "files") return folders.map((f) => f || $_("backup.other.rootFiles")).join(", ");
    if (c.count == null || c.reasons.some((r) => r.kind === "empty")) return "";
    switch (c.item) {
      case "settings":
        return $_("restore.other.settingsCount", { values: { count: o.settings.length || c.count } });
      case "contacts":
        return $_("backup.other.notes.contacts", { values: { count: c.count } }).split(".")[0];
      case "calls":
        return $_("backup.other.notes.calls", { values: { count: c.count } });
      case "wifi":
        return $_("backup.other.notes.wifi", { values: { count: c.count } });
      case "calendar":
        return $_("backup.other.notes.calendar", { values: { count: c.count } }).split(".")[0];
      default:
        return String(c.count);
    }
  }
</script>

<section>
  <ul>
    {#each items as c (c.item)}
      {@const blocked = c.status === "blocked"}
      {@const s = statuses[`other:${c.item}`]}
      <li class:blocked>
        <span class="icon" title={$_(`restore.status.${c.status}`)}>{ICON[c.status]}</span>
        <label>
          <input type="checkbox" checked={checked(c.item)} disabled={locked || blocked} onchange={(e) => set(c.item, e.currentTarget.checked)} />
          <span class="title">{title(c.item)}</span>
        </label>
        <span class="note">
          {count(c)}
          {#each c.reasons as r}<div class="reason" class:bad={blocked}>{reasonText(r)}</div>{/each}
        </span>
        <span class="status">{s ? $_(`backup.apps.status.${s}`) : ""}</span>
      </li>
      {#if c.item === "contacts" && !blocked && settings.advanced}
        <li class="sub">
          <label><input type="checkbox" bind:checked={o.contactsAsVcf} disabled={locked || !o.contacts} /> {$_("restore.other.contactsVcf")}</label>
        </li>
      {/if}
      {#if c.item === "files" && !blocked && folders.length > 1}
        <li class="sub">
          {#each folders as f}
            <label class="folder">
              <input
                type="checkbox"
                checked={o.folders.includes(f)}
                disabled={locked}
                onchange={(e) => (o.folders = e.currentTarget.checked ? [...o.folders, f] : o.folders.filter((x) => x !== f))}
              />
              {f || $_("backup.other.rootFiles")}
            </label>
          {/each}
        </li>
      {/if}
    {/each}
  </ul>
</section>

<style>
  section {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 0.6rem 1rem;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  li {
    display: grid;
    grid-template-columns: 1.5rem minmax(14rem, 18rem) 1fr auto;
    gap: 0.6rem;
    align-items: baseline;
    padding-block: 0.3rem;
    border-block-end: 1px solid var(--border);
  }
  li:last-child {
    border-block-end: none;
  }
  li.sub {
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem 1rem;
    padding-inline-start: 2.1rem;
  }
  li.blocked .title {
    opacity: 0.6;
  }
  .note {
    color: var(--muted);
    font-size: 0.9em;
  }
  .reason {
    color: var(--warn);
  }
  .reason.bad {
    color: var(--error);
  }
  .status {
    font-size: 0.85em;
    color: var(--accent);
  }
</style>
