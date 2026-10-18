<script lang="ts">
  import { _ } from "svelte-i18n";
  import type { CompatStatus, DataPart, ItemStatus, PreviewApp, Reason } from "$lib/bindings";
  import { restore, entryParts } from "$lib/restore.svelte";
  import { appName, permissionKeys, specialKey } from "$lib/backup.svelte";
  import AppIcon from "./AppIcon.svelte";
  import { settings } from "$lib/settings.svelte";

  let {
    apps,
    locked,
    statuses,
    targetSdk,
  }: { apps: PreviewApp[]; locked: boolean; statuses: Record<string, ItemStatus>; targetSdk?: number } = $props();

  let search = $state("");
  let filter: CompatStatus | "all" = $state("all");
  let why: string | null = $state(null);
  let expanded: string | null = $state(null);

  const ICON: Record<CompatStatus, string> = { ok: "✅", limited: "⚠️", blocked: "⛔" };

  const visible = $derived(
    apps.filter(
      (a) =>
        (!search.trim() ||
          a.meta.package.toLowerCase().includes(search.trim().toLowerCase()) ||
          (a.meta.label ?? "").toLowerCase().includes(search.trim().toLowerCase())) &&
        (filter === "all" || a.compat.status === filter),
    ),
  );

  function reasonText(r: Reason): string {
    if (r.kind === "roleTaken") {
      return $_("restore.reason.roleTaken", {
        values: { role: $_(`restore.roleNames.${r.role.split(".").pop()}`), holder: r.holder },
      });
    }
    return $_(`restore.reason.${r.kind}`);
  }

  function reasonDetail(r: Reason): string {
    const values = Object.fromEntries(Object.entries(r).map(([k, v]) => [k, Array.isArray(v) ? v.join(", ") : String(v)]));
    return $_(`restore.reasonDetail.${r.kind}`, { values });
  }

  function setAll(field: "app" | "data" | "permissions", value: boolean) {
    for (const a of apps) {
      const c = restore.choices[a.meta.package];
      if (!c || a.compat.status === "blocked") continue;
      if (field === "app") c.app = value && a.compat.action !== "dataOnly" ? value : c.app;
      else if (field === "data") c.data = value && a.compat.dataAllowed ? entryParts(a.meta.package) : [];
      else c.permissions = value && permissionKeys(a.meta.permissions).length > 0;
    }
  }

  function togglePart(pkg: string, part: DataPart, on: boolean) {
    const c = restore.choices[pkg];
    c.data = on ? [...new Set([...c.data, part])] : c.data.filter((p) => p !== part);
  }

  function toggleKey(pkg: string, key: string, on: boolean) {
    const c = restore.choices[pkg];
    c.excluded = on ? c.excluded.filter((k) => k !== key) : [...c.excluded, key];
  }

</script>

<div class="toolbar">
  <input type="search" placeholder={$_("backup.apps.search")} bind:value={search} />
  <label>
    {$_("restore.filter")}
    <select bind:value={filter}>
      <option value="all">{$_("restore.filterAll")}</option>
      <option value="ok">✅ {$_("restore.status.ok")}</option>
      <option value="limited">⚠️ {$_("restore.status.limited")}</option>
      <option value="blocked">⛔ {$_("restore.status.blocked")}</option>
    </select>
  </label>
  <span class="bulk">
    <button disabled={locked} onclick={() => setAll("data", true)}>{$_("backup.apps.dataAll")}</button>
    <button disabled={locked} onclick={() => setAll("data", false)}>{$_("backup.apps.dataNone")}</button>
    <button disabled={locked} onclick={() => setAll("permissions", true)}>{$_("backup.apps.permsAll")}</button>
    <button disabled={locked} onclick={() => setAll("permissions", false)}>{$_("backup.apps.permsNone")}</button>
  </span>
</div>

{#if apps.length === 0}
  <p class="muted">{$_("library.apps.none")}</p>
{:else}
  <table>
    <thead>
      <tr>
        <th></th>
        <th></th>
        {#if settings.advanced}
          <th>{$_("restore.advanced.versions")}</th>
          <th>{$_("restore.advanced.minSdk")}</th>
          <th>{$_("restore.advanced.abi")}</th>
        {/if}
        <th>{$_("restore.restoreApp")}</th>
        <th>{$_("restore.restoreData")}</th>
        <th>{$_("restore.permissions")}</th>
        {#if Object.keys(statuses).length}<th></th>{/if}
      </tr>
    </thead>
    <tbody>
      {#each visible as a (a.meta.package)}
        {@const c = restore.choices[a.meta.package]}
        {@const blocked = a.compat.status === "blocked"}
        {@const parts = entryParts(a.meta.package)}
        {@const hasPerms = permissionKeys(a.meta.permissions).length > 0}
        {@const status = statuses[a.meta.package]}
        {#if c}
          <tr class:blocked>
            <td class="icon" title={$_(`restore.status.${a.compat.status}`)}>{ICON[a.compat.status]}</td>
            <td class="name">
              <div class="title"><AppIcon app={a.meta} /><span>{appName(a.meta)}{#if a.meta.label && settings.advanced}{" "}<span class="mono muted small">{a.meta.package}</span>{/if}</span></div>
              <div class="muted small">{$_(`restore.action.${a.compat.action}`)}</div>
              {#each a.compat.reasons as r}
                <div class="reason small" class:bad={blocked}>{reasonText(r)}</div>
              {/each}
              {#if a.compat.reasons.length}
                <button class="link small" onclick={() => (why = why === a.meta.package ? null : a.meta.package)}>{$_("restore.why")}</button>
                {#if why === a.meta.package}
                  <ul class="why small">
                    {#each a.compat.reasons as r}
                      {#if reasonDetail(r)}<li>{reasonDetail(r)}</li>{/if}
                    {/each}
                  </ul>
                {/if}
              {/if}
            </td>
            {#if settings.advanced}
              <td class="mono small">
                {a.meta.versionCode} → {a.compat.installedVersion ?? $_("restore.advanced.notInstalled")}
                <div class="muted" title={$_("restore.advanced.signatures")}>
                  {a.meta.signatures.join(",") || "?"} / {a.compat.installedSignatures.join(",") || "—"}
                </div>
              </td>
              <td class="mono small">{a.meta.minSdk} / {targetSdk ?? "?"}</td>
              <td class="mono small">{a.meta.primaryAbi ?? "—"}</td>
            {/if}
            <td>
              <input
                type="checkbox"
                bind:checked={c.app}
                disabled={locked || blocked || a.compat.action === "dataOnly"}
                aria-label={$_("restore.restoreApp")}
              />
            </td>
            <td title={a.compat.dataAllowed ? "" : $_("restore.noData")}>
              {#if settings.advanced && a.compat.dataAllowed}
                <div class="parts">
                  {#each parts as part}
                    <label>
                      <input type="checkbox" checked={c.data.includes(part)} disabled={locked} onchange={(e) => togglePart(a.meta.package, part, e.currentTarget.checked)} />
                      {$_(`backup.apps.partsShort.${part}`)}
                    </label>
                  {/each}
                </div>
              {:else}
                <input
                  type="checkbox"
                  role="switch"
                  checked={c.data.length > 0}
                  disabled={locked || !a.compat.dataAllowed}
                  onchange={(e) => (c.data = e.currentTarget.checked ? parts : [])}
                  aria-label={$_("restore.restoreData")}
                />
              {/if}
            </td>
            <td>
              <input type="checkbox" bind:checked={c.permissions} disabled={locked || blocked || !hasPerms} aria-label={$_("restore.permissions")} />
              {#if settings.advanced && hasPerms}
                <button class="link small" onclick={() => (expanded = expanded === a.meta.package ? null : a.meta.package)}>
                  {permissionKeys(a.meta.permissions).length - c.excluded.length}/{permissionKeys(a.meta.permissions).length}
                </button>
              {/if}
            </td>
            {#if Object.keys(statuses).length}
              <td class="status {status ?? ''}">{status ? $_(`backup.apps.status.${status}`) : ""}</td>
            {/if}
          </tr>
          {#if settings.advanced && expanded === a.meta.package}
            {@const p = a.meta.permissions}
            <tr class="detail">
              <td colspan="20">
                <fieldset disabled={locked || !c.permissions}>
                  {#each [...p.runtime.map((r) => [`runtime:${r}`, r]), ...p.appOps.map((o) => [`op:${o.op}`, `${o.op}: ${o.mode}`]), ...p.special.map((s) => [specialKey(s), `${$_(`backup.special.${s.kind}`)}${"component" in s ? ` · ${s.component}` : ""}`]), ...p.roles.map((r) => [`role:${r}`, r])] as [key, label]}
                    <label class="mono small">
                      <input type="checkbox" checked={!c.excluded.includes(key)} onchange={(e) => toggleKey(a.meta.package, key, e.currentTarget.checked)} />
                      {label}
                    </label>
                  {/each}
                </fieldset>
              </td>
            </tr>
          {/if}
        {/if}
      {/each}
    </tbody>
  </table>
{/if}

<style>
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem 0.75rem;
    align-items: center;
    padding-block-end: 0.75rem;
  }
  .toolbar input[type="search"] {
    min-width: 12rem;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 0.3em 0.5em;
  }
  .bulk {
    display: flex;
    flex-wrap: wrap;
    gap: 0.25rem;
  }
  .bulk button {
    padding: 0.2em 0.6em;
    font-size: 0.9em;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    background: var(--surface);
    border: 1px solid var(--border);
  }
  th,
  td {
    padding: 0.4rem 0.6rem;
    text-align: start;
    border-block-end: 1px solid var(--border);
    vertical-align: top;
  }
  th {
    font-weight: 600;
    font-size: 0.9em;
    color: var(--muted);
    white-space: nowrap;
  }
  .icon {
    width: 1.5rem;
  }
  .title {
    display: flex;
    gap: 0.4rem;
    align-items: center;
  }
  td.mono {
    white-space: nowrap;
  }
  td.name {
    width: 45%;
    overflow-wrap: anywhere;
  }
  .small {
    font-size: 0.85em;
  }
  .reason {
    color: var(--warn);
  }
  .reason.bad {
    color: var(--error);
  }
  tr.blocked .mono {
    opacity: 0.7;
  }
  .why {
    margin: 0.2rem 0;
    padding-inline-start: 1.2rem;
    color: var(--muted);
  }
  .link {
    border: none;
    background: none;
    color: var(--accent);
    padding: 0;
  }
  .parts {
    display: flex;
    gap: 0.5rem;
    font-size: 0.85em;
    white-space: nowrap;
  }
  .detail td {
    background: var(--surface-2);
  }
  fieldset {
    border: none;
    display: flex;
    flex-wrap: wrap;
    gap: 0.2rem 1rem;
  }
  .status.done {
    color: var(--ok);
  }
  .status.error {
    color: var(--error);
  }
  .status.running {
    color: var(--accent);
  }
</style>
