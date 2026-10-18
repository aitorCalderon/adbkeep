<script lang="ts">
  import { _ } from "svelte-i18n";
  import { job as currentJob } from "$lib/job.svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { commands, type CoreError } from "$lib/bindings";
  import {
    backup,
    cancelResume,
    chosenSize,
    otherCategories,
    otherSize,
    passwordOk,
    selectable,
    simulateBackup,
    startBackup,
  } from "$lib/backup.svelte";
  import { settings } from "$lib/settings.svelte";
  import { formatSize } from "$lib/format";
  import { errorText } from "$lib/errors";
  import JobProgress from "./JobProgress.svelte";

  let { onSimulated }: { onSimulated: () => void } = $props();

  let freeOnPc: number | null = $state(null);
  let error: CoreError | null = $state(null);

  const apps = $derived(backup.analysis?.apps ?? []);
  const chosen = $derived(apps.filter((a) => backup.choices[a.package]?.app && selectable(a)));
  const withData = $derived(chosen.filter((a) => backup.choices[a.package].data.length > 0).length);
  const categories = $derived(otherCategories(backup.other));
  const size = $derived(
    chosen.reduce((s, a) => s + chosenSize(a, backup.choices[a.package]), 0) + otherSize(backup.other, backup.analysis?.other),
  );
  // Data is compressed, so this is an upper bound.
  // When resuming, part of it is already saved; the backup itself checks what is left.
  const missing = $derived(!backup.resume && freeOnPc != null && size > freeOnPc ? size - freeOnPc : 0);
  const IDLE = { running: false, packing: false, items: {}, bytesDone: 0, totalBytes: 0, current: null, speed: 0, result: null, error: null };
  const job = $derived(currentJob.kind === "backup" ? currentJob : IDLE);
  const ready = $derived(
    backup.analysis?.stage === "done" &&
      (chosen.length > 0 || categories > 0) &&
      (!!backup.resume || (backup.name.trim() && backup.destDir)) &&
      passwordOk(),
  );
  const mismatch = $derived(backup.encrypt && backup.passwordRepeat.length > 0 && backup.password !== backup.passwordRepeat);

  $effect(() => {
    const dir = backup.destDir;
    if (dir) commands.freeSpace(dir).then((f) => (freeOnPc = f));
  });

  async function chooseDir() {
    const dir = await open({ directory: true, defaultPath: backup.destDir || undefined });
    if (typeof dir === "string") backup.destDir = dir;
  }

  async function start() {
    error = await startBackup();
  }

  async function simulate() {
    error = await simulateBackup();
    if (!error) onSimulated();
  }

</script>

<footer>
  {#if job.running}
    <JobProgress note={$_("backup.action.appsClosed")} packingText={$_("backup.action.packing")} />
  {:else if !job.result && !job.error}
    <div class="row">
      <span class="summary">
        {$_("backup.action.summaryAll", { values: { apps: chosen.length, withData, categories, size: formatSize(size) } })}
      </span>
      {#if backup.resume}
        <span class="dest">
          {$_("backup.action.resuming", { values: { name: backup.resume.name } })}
          <button onclick={cancelResume}>{$_("backup.action.resumeCancel")}</button>
        </span>
      {:else}
      <span class="dest">
        <span class="muted">{$_("backup.action.destination")}:</span>
        <button class="path mono" onclick={chooseDir} title={backup.destDir}>📁 {backup.destDir || "…"}</button>
        <span class="muted">/</span>
        <input type="text" class="mono" aria-label={$_("backup.action.name")} bind:value={backup.name} />
        {#if freeOnPc != null}
          <span class:error-text={missing > 0} class:muted={missing === 0}>
            {missing > 0
              ? $_("backup.action.noSpace", { values: { size: formatSize(missing) } })
              : $_("backup.action.freeOnPc", { values: { size: formatSize(freeOnPc) } })}
          </span>
        {/if}
      </span>
      {/if}
    </div>
    <div class="row">
      <label><input type="checkbox" bind:checked={backup.pack} /> {$_("backup.action.pack")}</label>
      {#if backup.pack}
        <label><input type="checkbox" bind:checked={backup.encrypt} /> {$_("backup.action.encrypt")}</label>
        {#if backup.encrypt}
          <input type="password" autocomplete="new-password" placeholder={$_("backup.action.password")} aria-label={$_("backup.action.password")} bind:value={backup.password} />
          <input type="password" autocomplete="new-password" placeholder={$_("backup.action.passwordRepeat")} aria-label={$_("backup.action.passwordRepeat")} bind:value={backup.passwordRepeat} />
          {#if mismatch}
            <span class="error-text">{$_("backup.action.passwordMismatch")}</span>
          {:else}
            <span class="muted small">{$_("backup.action.passwordHelp")}</span>
          {/if}
        {/if}
      {/if}
      {#if backup.other?.wifi}<span class="warn-text">⚠️ {$_("backup.other.wifiWarning")}</span>{/if}
      {#if settings.advanced}
        <label>
          {$_("backup.action.compression")}
          <input type="number" min="1" max="19" bind:value={backup.compression} />
        </label>
      {/if}
      {#if error}<span class="error-text">{errorText(error)}</span>{/if}
      <span class="spacer"></span>
      {#if settings.advanced}
        <button disabled={!ready} onclick={simulate}>{$_("backup.action.simulate")}</button>
      {/if}
      <button class="primary" disabled={!ready || missing > 0} onclick={start}>
        {backup.resume ? $_("backup.action.resume") : $_("backup.action.start")}
      </button>
    </div>
  {/if}
</footer>

<style>
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
  .dest {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    flex: 1;
    min-width: 0;
  }
  .path {
    max-width: 22rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    padding: 0.2em 0.5em;
  }
  .dest input {
    min-width: 12rem;
    flex: 1;
  }
  input[type="number"] {
    width: 4rem;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 0.2em 0.4em;
  }
  .spacer {
    flex: 1;
  }
  .warn-text {
    color: var(--warn);
  }
  input[type="password"] {
    width: 11rem;
  }
  .small {
    font-size: 0.85em;
  }
</style>
