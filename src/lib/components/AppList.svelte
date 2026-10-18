<script lang="ts">
  import { _ } from "svelte-i18n";
  import type { AppInfo, DataPart, ItemStatus } from "$lib/bindings";
  import {
    apkSize,
    backup,
    chosenSize,
    existingParts,
    hasPermissions,
    partSize,
    selectable,
    permissionKeys,
    specialKey,
    appName,
    touch,
  } from "$lib/backup.svelte";
  import { settings } from "$lib/settings.svelte";
  import { formatSize } from "$lib/format";
  import { confirm } from "@tauri-apps/plugin-dialog";
  import AppIcon from "./AppIcon.svelte";

  let {
    apps,
    locked,
    statuses,
    system = false,
  }: { apps: AppInfo[]; locked: boolean; statuses: Record<string, ItemStatus>; system?: boolean } = $props();

  let search = $state("");
  let sort: "name" | "size" = $state("name");
  let onlyWarnings = $state(false);
  let expanded: string | null = $state(null);

  const visible = $derived.by(() => {
    const q = search.trim().toLowerCase();
    let list = apps.filter(
      (a) => (!q || a.package.toLowerCase().includes(q) || (a.label ?? "").toLowerCase().includes(q)) && (!onlyWarnings || a.keystore),
    );
    list = [...list].sort((a, b) => (sort === "size" ? fullSize(b) - fullSize(a) : appName(a).localeCompare(appName(b))));
    // Excluded system apps go last (ui-spec §3.2).
    return [...list.filter((a) => !a.exclusion), ...list.filter((a) => a.exclusion)];
  });

  const fullSize = (a: AppInfo) => apkSize(a) + existingParts(a).reduce((s, p) => s + partSize(a, p), 0);

  const selectedApps = $derived(apps.filter((a) => backup.choices[a.package]?.app && selectable(a)));
  const selectedSize = $derived(selectedApps.reduce((s, a) => s + chosenSize(a, backup.choices[a.package]), 0));

  function setAll(field: "app" | "data" | "permissions", value: boolean) {
    for (const a of apps) {
      const c = backup.choices[a.package];
      if (!c || !selectable(a)) continue;
      if (field === "app") c.app = value;
      else if (field === "permissions") c.permissions = value && hasPermissions(a);
      else {
        touch(a.package);
        c.data = value ? existingParts(a) : [];
      }
    }
  }

  function toggleData(app: AppInfo, on: boolean) {
    touch(app.package);
    backup.choices[app.package].data = on ? existingParts(app) : [];
  }

  function togglePart(app: AppInfo, part: DataPart, on: boolean) {
    touch(app.package);
    const c = backup.choices[app.package];
    c.data = on ? [...new Set([...c.data, part])] : c.data.filter((p) => p !== part);
  }

  function toggleKey(pkg: string, key: string, on: boolean) {
    const c = backup.choices[pkg];
    c.excluded = on ? c.excluded.filter((k) => k !== key) : [...c.excluded, key];
  }

  function roleTags(app: AppInfo): string[] {
    const tags = app.permissions.roles.map((r) => $_(`backup.roles.${r.split(".").pop()}`));
    if (app.permissions.special.some((s) => s.kind === "inputMethod" && s.default)) tags.push($_("backup.roles.keyboard"));
    return tags;
  }

  async function force(app: AppInfo) {
    const ok = await confirm($_("backup.apps.confirmForce", { values: { pkg: app.package } }), { kind: "warning" });
    if (!ok) return;
    backup.forced[app.package] = true;
    backup.choices[app.package].app = true;
    backup.choices[app.package].permissions = hasPermissions(app);
  }

  function exclusionText(app: AppInfo, technical: boolean) {
    const e = app.exclusion!;
    const values = e.kind === "systemSharedUser" ? { sharedUser: e.sharedUser } : {};
    return $_(`backup.apps.${technical ? "exclusionTech" : "exclusion"}.${e.kind}`, { values });
  }

  function permCount(app: AppInfo) {
    const p = app.permissions;
    return $_("backup.apps.permCount", {
      values: { perms: p.runtime.length + p.special.length + p.roles.length, ops: p.appOps.length },
    });
  }
</script>

<div class="toolbar">
  <input type="search" placeholder={$_("backup.apps.search")} bind:value={search} />
  <select bind:value={sort} aria-label={$_("backup.apps.sortName")}>
    <option value="name">{$_("backup.apps.sortName")}</option>
    <option value="size">{$_("backup.apps.sortSize")}</option>
  </select>
  <label><input type="checkbox" bind:checked={onlyWarnings} /> {$_("backup.apps.onlyWarnings")}</label>
  <span class="bulk">
    <button disabled={locked} onclick={() => setAll("app", true)}>{$_("backup.apps.all")}</button>
    <button disabled={locked} onclick={() => setAll("app", false)}>{$_("backup.apps.none")}</button>
    <button disabled={locked} onclick={() => setAll("data", true)}>{$_("backup.apps.dataAll")}</button>
    <button disabled={locked} onclick={() => setAll("data", false)}>{$_("backup.apps.dataNone")}</button>
    <button disabled={locked} onclick={() => setAll("permissions", true)}>{$_("backup.apps.permsAll")}</button>
    <button disabled={locked} onclick={() => setAll("permissions", false)}>{$_("backup.apps.permsNone")}</button>
  </span>
  <span class="total muted">
    {$_("backup.apps.selected", { values: { apps: selectedApps.length, size: formatSize(selectedSize) } })}
  </span>
</div>

{#if system}
  <p class="note">{$_("backup.apps.systemNote")}</p>
{/if}

{#if apps.length === 0}
  <p class="muted empty">{system ? $_("backup.apps.emptySystem") : $_("backup.apps.empty")}</p>
{:else}
  <div class="scroll">
  <table class:advanced={settings.advanced}>
    <thead>
      <tr>
        <th class="name"></th>
        {#if settings.advanced}
          <th>{$_("backup.apps.columns.version")}</th>
          <th class="num">{$_("backup.apps.columns.apk")}</th>
          <th class="num">{$_("backup.apps.columns.data")}</th>
          <th class="num">{$_("backup.apps.columns.splits")}</th>
          {#if system}
            <th>{$_("backup.apps.columns2.sharedUser")}</th>
            <th>{$_("backup.apps.columns2.signature")}</th>
          {/if}
        {/if}
        <th class="num">{$_("backup.apps.columns.size")}</th>
        <th>{$_("backup.apps.copyApp")}</th>
        <th>{$_("backup.apps.includeData")}</th>
        <th>{$_("backup.apps.permissions")}</th>
        {#if Object.keys(statuses).length}<th></th>{/if}
      </tr>
    </thead>
    <tbody>
      {#each visible as app (app.package)}
        {@const c = backup.choices[app.package]}
        {@const status = statuses[app.package]}
        {#if c && !selectable(app)}
          <tr class="excluded" id={`app-${app.package}`}>
            <td class="name">
              <div class="title"><AppIcon {app} /><span class="app-name">{appName(app)}{#if app.label && settings.advanced}{" "}<span class="mono muted pkg">{app.package}</span>{/if}</span></div>
              <div class="reason">{exclusionText(app, false)}</div>
              {#if settings.advanced}<div class="muted mono small">{exclusionText(app, true)}</div>{/if}
            </td>
            <td colspan="20" class="muted">
              {$_("backup.apps.excluded")}
              {#if settings.advanced}
                <button class="small-btn" disabled={locked} onclick={() => force(app)}>{$_("backup.apps.force")}</button>
              {/if}
            </td>
          </tr>
        {:else if c}
          <tr class:off={!c.app} id={`app-${app.package}`}>
            <td class="name">
              <div class="title">
                <AppIcon {app} />
                <span class="app-name">
                  {appName(app)}
                  {#if app.label && settings.advanced}<span class="mono muted pkg">{app.package}</span>{/if}
                </span>
                {#if app.keystore}
                  <span class="warn" title={$_("backup.apps.keystoreWarning")} aria-label={$_("backup.apps.keystoreWarning")}>⚠️</span>
                {/if}
              </div>
              {#each roleTags(app) as tag}<span class="tag">{tag}</span>{/each}
              {#if app.exclusion}<span class="tag forced">{$_("backup.apps.forced")}</span>{/if}
              {#if settings.advanced && system}
                <div class="muted mono small path" title={`${app.codePath}\n${app.flags.join(" ")}`}>{app.codePath}</div>
              {/if}
            </td>
            {#if settings.advanced}
              <td class="mono version">{app.versionName ?? ""} ({app.versionCode})</td>
              <td class="num">{formatSize(apkSize(app))}</td>
              <td class="num">{formatSize(fullSize(app) - apkSize(app))}</td>
              <td class="num">{app.apks.length}</td>
              {#if system}
                <td class="mono small">{app.sharedUser ?? "—"}</td>
                <td class="mono small">{app.signatures.join(",") || "—"}</td>
              {/if}
            {/if}
            <td class="num">{formatSize(chosenSize(app, c) || apkSize(app))}</td>
            <td><input type="checkbox" bind:checked={c.app} disabled={locked} aria-label={$_("backup.apps.copyApp")} /></td>
            <td>
              {#if settings.advanced}
                <div class="parts">
                  {#each existingParts(app) as part}
                    <label title={`${$_(`backup.apps.parts.${part}`)} · ${formatSize(partSize(app, part))}`}>
                      <input
                        type="checkbox"
                        checked={c.data.includes(part)}
                        disabled={locked || !c.app}
                        onchange={(e) => togglePart(app, part, e.currentTarget.checked)}
                      />
                      {$_(`backup.apps.partsShort.${part}`)}
                    </label>
                  {/each}
                </div>
              {:else}
                <input
                  type="checkbox"
                  role="switch"
                  checked={c.data.length > 0}
                  disabled={locked || !c.app || existingParts(app).length === 0}
                  onchange={(e) => toggleData(app, e.currentTarget.checked)}
                  aria-label={$_("backup.apps.includeData")}
                />
              {/if}
            </td>
            <td>
              <input
                type="checkbox"
                bind:checked={c.permissions}
                disabled={locked || !c.app || !hasPermissions(app)}
                aria-label={$_("backup.apps.permissions")}
              />
              {#if settings.advanced && hasPermissions(app)}
                <button class="link" onclick={() => (expanded = expanded === app.package ? null : app.package)}>
                  {permCount(app)}
                </button>
              {/if}
            </td>
            {#if Object.keys(statuses).length}
              <td class="status {status ?? ''}">{status ? $_(`backup.apps.status.${status}`) : ""}</td>
            {/if}
          </tr>
          {#if settings.advanced && expanded === app.package}
            {@const p = app.permissions}
            <tr class="detail">
              <td colspan="20">
                <div class="perm-groups">
                  {#each [["runtime", p.runtime.map((r) => [`runtime:${r}`, r])], ["appOps", p.appOps.map((o) => [`op:${o.op}`, `${o.op}: ${o.mode}`])], ["special", p.special.map((s) => [specialKey(s), `${$_(`backup.special.${s.kind}`)}${"component" in s ? ` · ${s.component}` : ""}`])], ["roles", p.roles.map((r) => [`role:${r}`, r])]] as [group, items]}
                    {#if items.length}
                      <fieldset disabled={locked || !c.permissions}>
                        <legend>{$_(`backup.apps.permDetail.${group}`)}</legend>
                        {#each items as [key, label]}
                          <label class="mono">
                            <input type="checkbox" checked={!c.excluded.includes(key)} onchange={(e) => toggleKey(app.package, key, e.currentTarget.checked)} />
                            {label}
                          </label>
                        {/each}
                      </fieldset>
                    {/if}
                  {/each}
                </div>
                <span class="muted mono">{permissionKeys(p).length - c.excluded.length}/{permissionKeys(p).length}</span>
              </td>
            </tr>
          {/if}
        {/if}
      {/each}
    </tbody>
  </table>
  </div>
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
  .total {
    margin-inline-start: auto;
  }
  .scroll {
    overflow-x: auto;
  }
  .version {
    min-width: 9rem;
    max-width: 14rem;
    overflow-wrap: anywhere;
    font-size: 0.85em;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }
  th,
  td {
    padding: 0.4rem 0.6rem;
    text-align: start;
    border-block-end: 1px solid var(--border);
    vertical-align: middle;
  }
  th {
    font-weight: 600;
    font-size: 0.9em;
    color: var(--muted);
    white-space: nowrap;
  }
  .num {
    text-align: end;
    white-space: nowrap;
  }
  td.name {
    width: 30%;
    min-width: 14rem;
    overflow-wrap: anywhere;
  }
  .title {
    display: flex;
    gap: 0.4rem;
    align-items: center;
  }
  .warn {
    cursor: help;
  }
  .app-name {
    display: flex;
    flex-direction: column;
  }
  .pkg {
    font-size: 0.8em;
  }
  .tag {
    display: inline-block;
    font-size: 0.8em;
    color: var(--accent);
    margin-inline-end: 0.5rem;
  }
  tr.off td:not(:nth-last-child(-n + 4)) {
    opacity: 0.55;
  }
  .parts {
    display: flex;
    gap: 0.6rem;
    font-size: 0.85em;
    white-space: nowrap;
  }
  .link {
    border: none;
    background: none;
    color: var(--accent);
    padding: 0 0.3rem;
    font-size: 0.85em;
    white-space: nowrap;
  }
  td:has(.link) {
    white-space: nowrap;
  }
  .detail td {
    background: var(--surface-2);
  }
  .perm-groups {
    display: flex;
    flex-wrap: wrap;
    gap: 0.75rem;
  }
  fieldset {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
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
  .empty {
    padding: 1rem;
  }
  .note {
    margin: 0 0 0.75rem;
    padding: 0.4rem 0.75rem;
    border-inline-start: 4px solid var(--warn);
    background: var(--surface);
  }
  tr.excluded td {
    opacity: 0.6;
  }
  tr.excluded .reason {
    font-size: 0.85em;
  }
  .small {
    font-size: 0.85em;
  }
  .small-btn {
    margin-inline-start: 0.5rem;
    padding: 0.1em 0.6em;
    font-size: 0.85em;
    opacity: 1;
  }
  .tag.forced {
    color: var(--warn);
  }
  .path {
    max-width: 20rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
