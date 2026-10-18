<script lang="ts">
  import { _ } from "svelte-i18n";
  import { save } from "@tauri-apps/plugin-dialog";
  import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
  import { commands, type CoreError, type JobResult } from "$lib/bindings";
  import { errorText } from "$lib/errors";
  import { formatSize } from "$lib/format";
  import { resetResult } from "$lib/backup.svelte";

  let { result, error, apps }: { result: JobResult | null; error: CoreError | null; apps: number } = $props();

  const title = $derived(
    error
      ? $_("backup.result.failed")
      : result?.cancelled
        ? $_("backup.result.cancelled")
        : result?.errors.length
          ? $_("backup.result.doneWithErrors")
          : $_("backup.result.done"),
  );
  const ok = $derived(!error && result && !result.cancelled && result.errors.length === 0);

  function seconds(ms: number) {
    return ms < 60000 ? `${Math.round(ms / 1000)} s` : `${Math.floor(ms / 60000)} min ${Math.round((ms % 60000) / 1000)} s`;
  }

  function reportText(): string {
    const lines = [title];
    if (result) {
      lines.push(`${$_("backup.result.path")}: ${result.path}`);
      lines.push($_("backup.result.summary", { values: { apps, size: formatSize(result.sizeOnDisk), time: seconds(result.durationMs) } }));
      for (const e of result.errors) lines.push(`- ${e.package ?? ""} ${e.step}: ${errorText(e.error)}`);
    }
    if (error) lines.push(errorText(error));
    return lines.join("\n");
  }

  let saveError = $state<string | null>(null);

  async function saveReport() {
    saveError = null;
    const path = await save({ defaultPath: "adbkeep-report.txt" });
    if (!path) return;
    const res = await commands.saveText(path, reportText());
    if (res.status === "error") saveError = errorText(res.error);
  }

  async function openFolder() {
    if (!result) return;
    if (result.path.endsWith(".adbkeep")) await revealItemInDir(result.path);
    else await openPath(result.path);
  }
</script>

<section class:ok class:bad={!ok}>
  <h3>{title}</h3>
  {#if error}
    <p class="error-text">{errorText(error)}</p>
  {/if}
  {#if result}
    <p>{$_("backup.result.summary", { values: { apps, size: formatSize(result.sizeOnDisk), time: seconds(result.durationMs) } })}</p>
    <p class="mono path">{$_("backup.result.path")}: {result.path}</p>
    {#if result.errors.length}
      <h4>{$_("backup.result.errors")}</h4>
      <ul>
        {#each result.errors as e}
          <li><span class="mono">{e.package} · {e.step}</span>: <span class="error-text">{errorText(e.error)}</span></li>
        {/each}
      </ul>
    {/if}
  {/if}
  <div class="actions">
    {#if result}<button onclick={openFolder}>{$_("backup.result.openFolder")}</button>{/if}
    <button onclick={saveReport}>{$_("backup.result.saveReport")}</button>
    {#if saveError}<span class="error-text">{saveError}</span>{/if}
    <button class="primary" onclick={resetResult}>{$_("backup.result.newBackup")}</button>
  </div>
</section>

<style>
  section {
    background: var(--surface);
    border: 1px solid var(--border);
    border-inline-start: 4px solid var(--warn);
    border-radius: var(--radius);
    padding: 0.75rem 1rem;
  }
  section.ok {
    border-inline-start-color: var(--ok);
  }
  h3 {
    margin: 0 0 0.4rem;
  }
  .path {
    overflow-wrap: anywhere;
  }
  .actions {
    display: flex;
    gap: 0.5rem;
    justify-content: flex-end;
    margin-block-start: 0.75rem;
  }
</style>
