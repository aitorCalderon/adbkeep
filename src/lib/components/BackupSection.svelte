<script lang="ts">
  import { _ } from "svelte-i18n";
  import { job as currentJob } from "$lib/job.svelte";
  import { tick } from "svelte";
  import { backup, ensureAnalysis, initDestination, type Tab } from "$lib/backup.svelte";
  import { device } from "$lib/device.svelte";
  import { settings } from "$lib/settings.svelte";
  import { errorText } from "$lib/errors";
  import AppList from "./AppList.svelte";
  import WarningsTab from "./WarningsTab.svelte";
  import ResultTab from "./ResultTab.svelte";
  import ActionBar from "./ActionBar.svelte";
  import OtherTab from "./OtherTab.svelte";

  const analysis = $derived(backup.analysis);
  const apps = $derived(analysis?.apps.filter((a) => a.kind === "user") ?? []);
  const systemApps = $derived(analysis?.apps.filter((a) => a.kind === "updatedSystem") ?? []);
  const IDLE = { running: false, packing: false, items: {}, bytesDone: 0, totalBytes: 0, current: null, speed: 0, result: null, error: null };
  const job = $derived(currentJob.kind === "backup" ? currentJob : IDLE);
  const finished = $derived(!!job.result || !!job.error);
  const locked = $derived(job.running || finished);
  const warningCount = $derived(
    [...apps, ...systemApps].filter((a) => a.keystore && !a.exclusion).length + (analysis?.accounts.length ?? 0),
  );

  let simulationOpen = $state(false);

  $effect(() => {
    if (device.snapshot?.state.kind === "ready") ensureAnalysis();
  });

  $effect(() => {
    const model = device.snapshot?.info?.model;
    if (model && !backup.name) initDestination(model);
  });

  const TABS: Tab[] = ["user", "system", "other", "warnings"];

  async function showApp(pkg: string) {
    backup.tab = systemApps.some((a) => a.package === pkg) ? "system" : "user";
    await tick();
    document.getElementById(`app-${pkg}`)?.scrollIntoView({ block: "center" });
  }
</script>

<div class="section">
  <div class="tabs" role="tablist">
    {#each TABS as t}
      <button role="tab" aria-selected={backup.tab === t} class:active={backup.tab === t} onclick={() => (backup.tab = t)}>
        {#if t === "warnings" && finished}
          {$_("backup.tabs.result")}
        {:else}
          {$_(`backup.tabs.${t}`)}
        {/if}
        {#if t === "warnings" && !finished && warningCount > 0}<span class="badge">{warningCount}</span>{/if}
      </button>
    {/each}
    {#if analysis && analysis.stage !== "done"}
      <span class="analysing"><span class="spinner"></span>{$_(`backup.analysis.stage.${analysis.stage}`)}</span>
    {:else if !analysis}
      <span class="analysing"><span class="spinner"></span>{$_("backup.analysis.running")}</span>
    {:else if settings.advanced && !job.running}
      <button class="small" onclick={() => ensureAnalysis(true)}>{$_("backup.analysis.retry")}</button>
    {/if}
  </div>

  <div class="content">
    {#each analysis?.errors ?? [] as e}
      <p class="error-text">{$_("backup.analysis.stageFailed", { values: { error: errorText(e.error) } })}</p>
    {/each}

    {#if backup.tab === "user"}
      {#if analysis}
        <AppList {apps} {locked} statuses={job.items} />
        {#if settings.advanced}
          <fieldset class="options" disabled={locked}>
            <legend>{$_("backup.apps.options")}</legend>
            <label><input type="checkbox" bind:checked={backup.includeNoBackup} /> {$_("backup.apps.includeNoBackup")}</label>
          </fieldset>
        {/if}
      {/if}
    {:else if backup.tab === "other"}
      {#if analysis?.other && backup.other}
        <OtherTab a={analysis.other} sel={backup.other} {locked} statuses={job.items} />
      {:else if analysis && analysis.stage === "done"}
        <p class="muted">{$_("backup.other.nothing")}</p>
      {/if}
    {:else if backup.tab === "system"}
      {#if analysis}
        <AppList apps={systemApps} {locked} statuses={job.items} system />
      {/if}
    {:else if backup.tab === "warnings"}
      {#if finished}
        <ResultTab result={job.result} error={job.error} apps={Object.keys(job.items).length} />
      {:else if analysis}
        <WarningsTab apps={[...apps, ...systemApps.filter((a) => !a.exclusion)]} accounts={analysis.accounts} onShowApp={showApp} />
      {/if}
    {:else}
      <p class="muted">{$_("section.comingSoon")}</p>
    {/if}
  </div>

  <ActionBar onSimulated={() => (simulationOpen = true)} />
</div>

{#if simulationOpen && backup.simulation}
  <div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && (simulationOpen = false)}>
    <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="sim-title">
      <h3 id="sim-title">{$_("backup.action.simulationTitle")}</h3>
      <pre class="mono">{backup.simulation.join("\n")}</pre>
      <div class="actions">
        <button onclick={() => navigator.clipboard.writeText(backup.simulation?.join("\n") ?? "")}>{$_("backup.action.copy")}</button>
        <button class="primary" onclick={() => (simulationOpen = false)}>{$_("backup.action.close")}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .section {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .tabs {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    padding: 0.5rem 1rem 0;
    border-block-end: 1px solid var(--border);
  }
  .tabs button[role="tab"] {
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
  .analysing {
    margin-inline-start: auto;
    display: flex;
    align-items: center;
    gap: 0.4rem;
    color: var(--muted);
  }
  .small {
    margin-inline-start: auto;
    font-size: 0.85em;
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
    width: min(1000px, 94vw);
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
