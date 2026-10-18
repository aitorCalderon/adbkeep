<script lang="ts">
  import { onMount } from "svelte";
  import { _ } from "svelte-i18n";
  import { library, startLibrary } from "$lib/library.svelte";
  import { loadPreview, otherRestoreCount, restore, simulateRestore, startRestore, type RestoreTab } from "$lib/restore.svelte";
  import { job } from "$lib/job.svelte";
  import { device } from "$lib/device.svelte";
  import { settings } from "$lib/settings.svelte";
  import { errorText } from "$lib/errors";
  import { formatDate, formatSize } from "$lib/format";
  import type { CoreError } from "$lib/bindings";
  import RestoreAppList from "./RestoreAppList.svelte";
  import RestoreWarnings from "./RestoreWarnings.svelte";
  import RestoreOtherTab from "./RestoreOtherTab.svelte";
  import RestoreResult from "./RestoreResult.svelte";
  import JobProgress from "./JobProgress.svelte";
  import UnlockForm from "./UnlockForm.svelte";
  import { refreshLibrary } from "$lib/library.svelte";

  onMount(async () => {
    await startLibrary();
    if (!restore.backupPath) {
      const first = library.backups.find((b) => b.status.kind === "complete" || b.status.kind === "incomplete");
      if (first) await loadPreview(first.path);
    } else if (!restore.preview && !restore.loading) {
      await loadPreview();
    }
  });

  // Encrypted packs are listed too: picking one asks for its password.
  const usable = $derived(
    library.backups.filter((b) => b.status.kind === "complete" || b.status.kind === "incomplete" || b.status.kind === "encrypted"),
  );
  const needsPassword = $derived(restore.error?.code === "passwordRequired" || restore.error?.code === "wrongPassword");
  /** Name of the single item chosen from Mis copias. */
  const onlyName = $derived.by(() => {
    const o = restore.only;
    if (!o) return null;
    if (o.kind === "other") return $_(`backup.other.items.${o.item}`);
    const a = restore.preview?.apps.find((x) => x.meta.package === o.package);
    return a?.meta.label || o.package;
  });

  async function unlocked() {
    await refreshLibrary();
    await loadPreview();
  }
  const mine = $derived(job.kind === "restore");
  const finished = $derived(mine && (!!job.result || !!job.error));
  const running = $derived(job.running);
  const locked = $derived(running || finished);
  const statuses = $derived(mine ? job.items : {});
  const apps = $derived(restore.preview?.apps ?? []);
  const userApps = $derived(apps.filter((a) => a.meta.kind === "user"));
  const systemApps = $derived(apps.filter((a) => a.meta.kind === "updatedSystem"));
  const chosen = $derived(apps.filter((a) => {
    const c = restore.choices[a.meta.package];
    return c && a.compat.status !== "blocked" && (c.app || c.data.length > 0 || c.permissions);
  }));
  const withData = $derived(chosen.filter((a) => restore.choices[a.meta.package].data.length > 0).length);
  const categories = $derived(otherRestoreCount(restore.other));
  const warningCount = $derived(
    apps.filter((a) => a.compat.reasons.length > 0).length +
      (restore.preview?.other.filter((o) => o.status !== "ok" && !o.reasons.some((r) => r.kind === "empty")).length ?? 0),
  );
  const info = $derived(device.snapshot?.info ?? null);

  let error: CoreError | null = $state(null);
  let simulationOpen = $state(false);

  const TABS: RestoreTab[] = ["user", "system", "other", "warnings"];

  async function start() {
    error = await startRestore();
  }

  async function simulate() {
    error = await simulateRestore();
    if (!error) simulationOpen = true;
  }

  async function toggleBypass(on: boolean) {
    restore.bypassLowTargetSdk = on;
    await loadPreview();
  }
</script>

<div class="section">
  <div class="header">
    <label class="pick">
      <span class="muted">{$_("restore.chooseBackup")}</span>
      <select
        value={restore.backupPath ?? ""}
        disabled={locked}
        onchange={(e) => loadPreview(e.currentTarget.value || null)}
      >
        {#each usable as b}
          <option value={b.path}>
            {b.encrypted ? "🔒 " : ""}{b.name}{#if b.manifest}{" — "}{b.manifest.source.model} · Android {b.manifest.source.androidRelease} · {formatDate(b.manifest.createdAt)} · {formatSize(b.size)}{/if}
          </option>
        {/each}
      </select>
    </label>
    <span class="arrow">→</span>
    <span class="target">
      <span class="muted">{$_("restore.target")}</span>
      <strong>{info?.model ?? "?"}</strong> · Android {info?.androidRelease ?? "?"}
    </span>
  </div>

  {#if usable.length === 0 && !library.loading}
    <p class="muted pad">{$_("restore.noBackups")}</p>
  {:else}
    <div class="tabs" role="tablist">
      {#each TABS as t}
        <button role="tab" aria-selected={restore.tab === t} class:active={restore.tab === t} onclick={() => (restore.tab = t)}>
          {t === "warnings" && finished ? $_("backup.tabs.result") : $_(`backup.tabs.${t}`)}
          {#if t === "warnings" && !finished && warningCount > 0}<span class="badge">{warningCount}</span>{/if}
        </button>
      {/each}
      {#if restore.loading}<span class="loading"><span class="spinner"></span>{$_("restore.loading")}</span>{/if}
    </div>

    <div class="content">
      {#if needsPassword && restore.backupPath}
        <p>{$_("library.statusHelp.encrypted")}</p>
        {#key restore.backupPath}<UnlockForm path={restore.backupPath} onUnlocked={unlocked} />{/key}
      {:else if restore.error}<p class="error-text">{errorText(restore.error)}</p>{/if}
      {#if onlyName && restore.preview && !locked}
        <p class="only">{$_("restore.only", { values: { item: onlyName } })}</p>
      {/if}
      {#if restore.tab === "warnings"}
        {#if finished}
          <RestoreResult result={job.result} error={job.error} manifest={restore.preview?.manifest ?? null} />
        {:else}
          <RestoreWarnings {apps} />
        {/if}
      {:else if restore.tab === "other"}
        {#if restore.preview && restore.preview.other.length}
          <RestoreOtherTab items={restore.preview.other} {locked} {statuses} />
        {:else if restore.preview}
          <p class="muted">{$_("backup.other.nothing")}</p>
        {/if}
      {:else if restore.preview}
        <RestoreAppList apps={restore.tab === "user" ? userApps : systemApps} {locked} {statuses} targetSdk={info?.sdk} />
        {#if settings.advanced}
          <fieldset class="options" disabled={locked}>
            <legend>{$_("backup.apps.options")}</legend>
            <label><input type="checkbox" bind:checked={restore.installAsPlay} /> {$_("restore.advanced.installAsPlay")}</label>
            <label>
              <input type="checkbox" checked={restore.bypassLowTargetSdk} onchange={(e) => toggleBypass(e.currentTarget.checked)} />
              {$_("restore.advanced.bypassTargetSdk")}
            </label>
          </fieldset>
        {/if}
      {:else if !restore.loading}
        <p class="muted">{$_("restore.pickBackup")}</p>
      {/if}
    </div>

    <footer>
      {#if running && mine}
        <JobProgress note={$_("restore.dataRestoreNote")} packingText={$_("restore.unpacking")} />
      {:else if !finished}
        <div class="row">
          <span class="summary">{$_("restore.summaryAll", { values: { apps: chosen.length, withData, categories } })}</span>
          {#if error}<span class="error-text">{errorText(error)}</span>{/if}
          <span class="spacer"></span>
          {#if settings.advanced}
            <button disabled={(!chosen.length && !categories) || running} onclick={simulate}>{$_("backup.action.simulate")}</button>
          {/if}
          <button class="primary" disabled={(!chosen.length && !categories) || running} onclick={start}>{$_("restore.start")}</button>
        </div>
      {/if}
    </footer>
  {/if}
</div>

{#if simulationOpen && restore.simulation}
  <div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && (simulationOpen = false)}>
    <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="rsim-title">
      <h3 id="rsim-title">{$_("backup.action.simulationTitle")}</h3>
      <pre class="mono">{restore.simulation.join("\n")}</pre>
      <div class="actions">
        <button onclick={() => navigator.clipboard.writeText(restore.simulation?.join("\n") ?? "")}>{$_("backup.action.copy")}</button>
        <button class="primary" onclick={() => (simulationOpen = false)}>{$_("backup.action.close")}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .only {
    border-inline-start: 4px solid var(--accent);
    background: var(--surface);
    padding: 0.35rem 0.75rem;
    margin-block: 0 0.5rem;
  }
  .section {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .header {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem 1rem;
    padding: 0.6rem 1rem;
    background: var(--surface);
    border-block-end: 1px solid var(--border);
  }
  .pick {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    flex: 1;
    min-width: 0;
  }
  .pick select {
    flex: 1;
    min-width: 0;
  }
  .arrow {
    color: var(--muted);
    font-size: 1.2rem;
  }
  .target {
    display: flex;
    gap: 0.4rem;
    align-items: baseline;
  }
  .pad {
    padding: 1rem;
  }
  .tabs {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    padding: 0.5rem 1rem 0;
    border-block-end: 1px solid var(--border);
  }
  .tabs button {
    border: 1px solid transparent;
    border-block-end: none;
    border-radius: var(--radius) var(--radius) 0 0;
    background: transparent;
  }
  .tabs button.active {
    background: var(--surface);
    border-color: var(--border);
    font-weight: 600;
    margin-block-end: -1px;
  }
  .badge {
    display: inline-block;
    min-width: 1.3em;
    margin-inline-start: 0.3em;
    padding: 0 0.35em;
    border-radius: 1em;
    background: var(--warn);
    color: #fff;
    font-size: 0.8em;
    text-align: center;
  }
  .loading {
    margin-inline-start: auto;
    display: flex;
    gap: 0.4rem;
    align-items: center;
    color: var(--muted);
  }
  .content {
    flex: 1;
    overflow: auto;
    padding: 0.75rem 1rem;
    min-height: 0;
  }
  .options {
    margin-block-start: 0.75rem;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
  footer {
    background: var(--surface);
    border-block-start: 1px solid var(--border);
    padding: 0.5rem 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.4rem 1rem;
  }
  .summary {
    font-weight: 600;
  }
  .spacer {
    flex: 1;
  }
  .spinner {
    width: 0.9rem;
    height: 0.9rem;
    border: 2px solid var(--border);
    border-block-start-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgb(0 0 0 / 0.35);
    display: grid;
    place-items: center;
  }
  .dialog {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 1rem 1.25rem;
    width: min(1100px, 94vw);
    max-height: 85vh;
    display: flex;
    flex-direction: column;
  }
  .dialog pre {
    flex: 1;
    overflow: auto;
    background: var(--surface-2);
    padding: 0.5rem;
    border-radius: var(--radius);
    white-space: pre;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }
</style>
