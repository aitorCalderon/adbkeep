<script lang="ts">
  import { _ } from "svelte-i18n";
  import { save } from "@tauri-apps/plugin-dialog";
  import { commands } from "$lib/bindings";
  import { clearLog, commandLine, entryText, log } from "$lib/log.svelte";
  import { formatSize, formatTime } from "$lib/format";
  import { errorText } from "$lib/errors";

  let open = $state(false);
  let filter = $state("");

  const visible = $derived(
    filter.trim()
      ? log.entries.filter((e) => entryText(e).toLowerCase().includes(filter.trim().toLowerCase()))
      : log.entries,
  );
  const text = () => visible.map(entryText).join("\n\n");

  let saveError = $state<string | null>(null);

  async function saveLog() {
    saveError = null;
    const path = await save({ defaultPath: "adbkeep-log.txt" });
    if (!path) return;
    const res = await commands.saveText(path, text());
    if (res.status === "error") saveError = errorText(res.error);
  }
</script>

<section class="log" class:open>
  <div class="bar">
    <button class="toggle" onclick={() => (open = !open)} aria-expanded={open}>
      {open ? "▾" : "▸"} {$_("log.title")} <span class="muted">({log.entries.length})</span>
    </button>
    {#if open}
      <input type="text" placeholder={$_("log.filter")} bind:value={filter} />
      <button onclick={() => navigator.clipboard.writeText(text())}>{$_("log.copy")}</button>
      <button onclick={saveLog}>{$_("log.save")}</button>
      {#if saveError}<span class="error-text">{saveError}</span>{/if}
      <button onclick={clearLog}>{$_("log.clear")}</button>
    {/if}
  </div>
  {#if open}
    <div class="entries mono">
      {#each visible as e (e.id)}
        <details class:failed={e.exitCode !== 0}>
          <summary>
            <span class="muted">{formatTime(e.startedAt)}</span>
            <span class="cmd">$ {commandLine(e)}</span>
            <span class="code">{$_("log.exitCode", { values: { code: e.exitCode ?? "?" } })} · {e.durationMs} ms</span>
            {#if e.streamedBytes != null}
              <span class="muted">{$_("log.streamed", { values: { size: formatSize(e.streamedBytes) } })}</span>
            {/if}
          </summary>
          {#if e.stdout}<pre>{e.stdout}</pre>{/if}
          {#if e.stderr}<pre class="error-text">{e.stderr}</pre>{/if}
        </details>
      {:else}
        <p class="muted">{$_("log.empty")}</p>
      {/each}
    </div>
  {/if}
</section>

<style>
  .log {
    border-block-start: 1px solid var(--border);
    background: var(--surface);
  }
  .log.open {
    height: 35vh;
    display: flex;
    flex-direction: column;
  }
  .bar {
    display: flex;
    gap: 0.5rem;
    align-items: center;
    padding: 0.3rem 1rem;
  }
  .toggle {
    border: none;
    background: none;
    padding-inline-start: 0;
    margin-inline-end: auto;
  }
  .entries {
    flex: 1;
    overflow: auto;
    padding: 0 1rem 0.5rem;
  }
  summary {
    display: flex;
    gap: 0.75rem;
    cursor: pointer;
    white-space: nowrap;
  }
  .cmd {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .failed .code {
    color: var(--error);
  }
  pre {
    margin: 0.25rem 0 0.5rem 1.5rem;
    white-space: pre-wrap;
    word-break: break-all;
  }
</style>
