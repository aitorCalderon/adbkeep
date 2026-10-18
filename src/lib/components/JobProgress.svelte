<script lang="ts">
  import { _ } from "svelte-i18n";
  import { commands } from "$lib/bindings";
  import { job } from "$lib/job.svelte";
  import { settings } from "$lib/settings.svelte";
  import { formatSize } from "$lib/format";

  /** Line under the progress bar, e.g. "Las apps se cierran mientras se copian sus datos". */
  let { note, packingText }: { note: string; packingText: string } = $props();

  const percent = $derived(job.totalBytes ? Math.min(100, (job.bytesDone / job.totalBytes) * 100) : 0);
</script>

{#if job.paused}
  <div class="paused" role="alert">
    ⏸ {$_(`backup.action.paused.${job.paused}`)}
  </div>
{/if}
<div class="progress-row">
  <progress max="100" value={percent}></progress>
  <span class="mono">{Math.floor(percent)} %</span>
</div>
<div class="row">
  <span>
    {#if job.packing}
      {packingText}
    {:else}
      {$_("backup.action.progress", { values: { done: formatSize(job.bytesDone), total: formatSize(job.totalBytes) } })}
      {#if job.current}· <span class="mono">{job.current.package}</span>{/if}
      {#if settings.advanced && job.current}
        · <span class="mono">{job.current.step}</span>
        {#if job.speed}· {$_("backup.action.speed", { values: { speed: formatSize(job.speed) } })}{/if}
      {/if}
    {/if}
  </span>
  <strong class="warn-text">{$_("backup.action.dontDisconnect")}</strong>
  <span class="muted">{note}</span>
  <button onclick={() => commands.cancelJob()}>{job.paused ? $_("backup.action.pausedCancel") : $_("backup.action.cancel")}</button>
</div>

<style>
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.4rem 1rem;
  }
  .progress-row {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }
  progress {
    flex: 1;
    height: 0.8rem;
    accent-color: var(--accent);
  }
  .warn-text {
    color: var(--warn);
  }
  .paused {
    border-inline-start: 4px solid var(--warn);
    background: var(--surface-2);
    padding: 0.4rem 0.75rem;
    font-weight: 600;
  }
</style>
