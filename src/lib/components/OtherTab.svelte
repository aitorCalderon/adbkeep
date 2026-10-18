<script lang="ts">
  import { _ } from "svelte-i18n";
  import type { ItemStatus, OtherAnalysis, OtherSelection } from "$lib/bindings";
  import { isWhitelisted } from "$lib/backup.svelte";
  import { settings } from "$lib/settings.svelte";
  import { formatSize } from "$lib/format";

  let {
    a,
    sel,
    locked,
    statuses,
  }: { a: OtherAnalysis; sel: OtherSelection; locked: boolean; statuses: Record<string, ItemStatus> } = $props();

  const helper = $derived(settings.useHelper);
  const contactsTotal = $derived(a.contacts.reduce((s, c) => s + c.count, 0));
  const localContacts = $derived(a.contacts.filter((c) => c.local).map((c) => c.accountType));
  const events = $derived(a.calendars.reduce((s, c) => s + c.events, 0));
  const personal = $derived(sel.sounds || sel.wallpaper || sel.dictionary);

  let settingsSearch = $state("");
  const settingsShown = $derived(
    a.settings.filter((s) => !settingsSearch.trim() || `${s.table}/${s.key}`.includes(settingsSearch.trim())),
  );

  function status(key: string) {
    const s = statuses[`other:${key}`];
    return s ? $_(`backup.apps.status.${s}`) : "";
  }

  function setPersonal(on: boolean) {
    sel.sounds = on && a.soundsSize > 0;
    sel.wallpaper = on && a.wallpaperFiles.length > 0;
    sel.dictionary = on && helper && (a.dictionary ?? 0) > 0;
  }

  function toggleSetting(id: string, on: boolean) {
    const list = sel.settings ?? [];
    sel.settings = on ? [...list, id] : list.filter((k) => k !== id);
  }

  function toggleFolder(name: string, on: boolean) {
    sel.folders = on ? [...sel.folders, name] : sel.folders.filter((f) => f !== name);
  }

  function toggleAccount(type: string, on: boolean) {
    sel.contacts = on ? [...sel.contacts, type] : sel.contacts.filter((t) => t !== type);
  }
</script>

{#snippet row(key: string, checked: boolean, onchange: (on: boolean) => void, note: string, needsHelper = false, empty = false)}
  <li>
    <label class:disabled={(needsHelper && !helper) || empty}>
      <input
        type="checkbox"
        {checked}
        disabled={locked || (needsHelper && !helper) || empty}
        onchange={(e) => onchange(e.currentTarget.checked)}
      />
      <span class="title">{$_(`backup.other.items.${key}`)}</span>
    </label>
    <span class="note">
      {#if needsHelper && !helper}{$_("backup.other.needsHelper")}{:else}{note}{/if}
    </span>
    <span class="status">{status(key)}</span>
  </li>
{/snippet}

<section>
  <h3>{$_("backup.other.groups.communications")}</h3>
  <ul>
    {@render row("sms", sel.sms, (on) => (sel.sms = on), $_("backup.other.notes.sms", { values: { messages: (a.sms ?? 0) + (a.mms ?? 0), threads: a.threads ?? 0 } }), true)}
    {@render row(
      "contacts",
      sel.contacts.length > 0,
      (on) => (sel.contacts = on ? localContacts : []),
      $_("backup.other.notes.contacts", { values: { count: a.contacts.filter((c) => sel.contacts.includes(c.accountType)).reduce((s, c) => s + c.count, 0) || contactsTotal } }),
      true,
      contactsTotal === 0,
    )}
    {#if settings.advanced && a.contacts.length && helper}
      <li class="sub">
        <fieldset disabled={locked}>
          <legend>{$_("backup.other.advanced.accounts")}</legend>
          {#each a.contacts as c}
            <label class="mono small">
              <input type="checkbox" checked={sel.contacts.includes(c.accountType)} onchange={(e) => toggleAccount(c.accountType, e.currentTarget.checked)} />
              {c.accountType || $_("backup.other.advanced.local")} ({c.count})
            </label>
          {/each}
        </fieldset>
      </li>
    {/if}
    {@render row("calls", sel.calls, (on) => (sel.calls = on), $_("backup.other.notes.calls", { values: { count: a.calls ?? 0 } }), true)}
    {@render row("blocked", sel.blocked, (on) => (sel.blocked = on), $_("backup.other.notes.blocked", { values: { count: a.blocked ?? 0 } }), true)}
  </ul>
</section>

<section>
  <h3>{$_("backup.other.groups.organization")}</h3>
  <ul>
    {@render row("calendar", sel.calendar, (on) => (sel.calendar = on), $_("backup.other.notes.calendar", { values: { count: events } }), true, a.calendars.length === 0)}
  </ul>
</section>

<section>
  <h3>{$_("backup.other.groups.system")}</h3>
  <ul>
    {@render row("wifi", sel.wifi, (on) => (sel.wifi = on), $_("backup.other.notes.wifi", { values: { count: a.wifiNetworks ?? 0 } }), false, !a.wifiNetworks)}
    {@render row(
      "settings",
      sel.settings !== null,
      (on) => (sel.settings = on ? a.settings.filter((s) => isWhitelisted(s.table, s.key)).map((s) => `${s.table}/${s.key}`) : null),
      $_("backup.other.notes.settings"),
    )}
    {#if settings.advanced && sel.settings !== null}
      <li class="sub">
        <fieldset disabled={locked}>
          <legend>{$_("backup.other.advanced.settingsTable")} ({sel.settings.length}/{a.settings.length})</legend>
          <p class="muted small">{$_("backup.other.advanced.settingsHint")}</p>
          <input type="search" placeholder={$_("backup.other.advanced.search")} bind:value={settingsSearch} />
          <div class="settings-list">
            {#each settingsShown as s (`${s.table}/${s.key}`)}
              {@const id = `${s.table}/${s.key}`}
              <label class="mono small" class:white={isWhitelisted(s.table, s.key)}>
                <input type="checkbox" checked={sel.settings.includes(id)} onchange={(e) => toggleSetting(id, e.currentTarget.checked)} />
                {id} = <span class="value">{s.value}</span>
              </label>
            {/each}
          </div>
        </fieldset>
      </li>
    {/if}
    {@render row("notifications", sel.notifications, (on) => (sel.notifications = on), $_("backup.other.notes.notifications"), false, !a.notificationPolicy)}
    {@render row("personal", personal, setPersonal, $_("backup.other.notes.personal", { values: { words: a.dictionary ?? 0 } }))}
    {#if a.certificates > 0}
      {@render row("certificates", sel.certificates, (on) => (sel.certificates = on), $_("backup.other.notes.certificates", { values: { count: a.certificates } }))}
    {/if}
  </ul>
</section>

<section>
  <h3>{$_("backup.other.groups.files")} <span class="status">{status("files")}</span></h3>
  <ul class="folders">
    {#each a.folders.filter((f) => settings.advanced || !f.name.startsWith(".")) as f (f.name)}
      <li>
        <label>
          <input type="checkbox" checked={sel.folders.includes(f.name)} disabled={locked} onchange={(e) => toggleFolder(f.name, e.currentTarget.checked)} />
          <span class="title">{f.name || $_("backup.other.rootFiles")}</span>
        </label>
        <span class="note">
          {formatSize(f.size)}{#if f.name.startsWith(".")} · {$_("backup.other.hidden")}{/if}
        </span>
      </li>
    {/each}
  </ul>
  {#if settings.advanced}
    <label class="exclude">
      {$_("backup.other.advanced.exclude")}
      <input
        type="text"
        class="mono"
        disabled={locked}
        value={sel.exclude.join(", ")}
        onchange={(e) => (sel.exclude = e.currentTarget.value.split(",").map((p) => p.trim()).filter(Boolean))}
      />
    </label>
  {/if}
</section>

{#if settings.advanced}
  <section class="info">
    <p class="small">{$_("backup.other.advanced.method")}</p>
    <p class="small">{$_("backup.other.advanced.users")}</p>
    {#if a.databases.length}
      <h4>{$_("backup.other.advanced.databases")}</h4>
      <ul class="mono small">
        {#each a.databases as d}<li>{d.path} · {formatSize(d.size)}</li>{/each}
      </ul>
    {/if}
  </section>
{/if}

<style>
  section {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 0.6rem 1rem;
    margin-block-end: 0.75rem;
  }
  h3 {
    margin: 0 0 0.4rem;
    font-size: 1rem;
  }
  h4 {
    margin: 0.5rem 0 0.2rem;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  li {
    display: grid;
    grid-template-columns: minmax(14rem, 18rem) 1fr auto;
    gap: 0.75rem;
    align-items: baseline;
    padding-block: 0.25rem;
  }
  li.sub {
    display: block;
    padding-inline-start: 1.6rem;
  }
  .folders li {
    grid-template-columns: minmax(14rem, 18rem) 1fr;
  }
  .disabled {
    opacity: 0.55;
  }
  .note {
    color: var(--muted);
    font-size: 0.9em;
  }
  .status {
    font-size: 0.85em;
    color: var(--accent);
  }
  .small {
    font-size: 0.85em;
  }
  fieldset {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
  }
  .settings-list {
    max-height: 16rem;
    overflow: auto;
    display: flex;
    flex-direction: column;
  }
  .settings-list label {
    white-space: nowrap;
  }
  .white {
    font-weight: 600;
  }
  .value {
    color: var(--muted);
  }
  .exclude {
    display: flex;
    gap: 0.5rem;
    align-items: center;
    margin-block-start: 0.5rem;
  }
  .exclude input {
    flex: 1;
  }
  input[type="search"] {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 0.2em 0.4em;
    max-width: 20rem;
  }
  .info p {
    margin: 0.2rem 0;
  }
</style>
