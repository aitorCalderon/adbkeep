<script lang="ts">
  import { _ } from "svelte-i18n";
  import { save } from "@tauri-apps/plugin-dialog";
  import { commands, type CoreError, type JobResult, type Manifest } from "$lib/bindings";
  import { errorText } from "$lib/errors";
  import { settings } from "$lib/settings.svelte";
  import { resetRestore, restore } from "$lib/restore.svelte";

  let { result, error, manifest }: { result: JobResult | null; error: CoreError | null; manifest: Manifest | null } = $props();

  const title = $derived(
    error
      ? $_("restore.result.failed")
      : result?.cancelled
        ? $_("restore.result.cancelled")
        : result?.errors.length
          ? $_("restore.result.doneWithErrors")
          : $_("restore.result.done"),
  );
  const ok = $derived(!error && result && !result.cancelled && result.errors.length === 0);
  const restoredKeystore = $derived(
    (manifest?.keystorePackages ?? []).filter((p) => result?.restored.some((r) => r.package === p && r.dataParts.length > 0)),
  );

  /** ui-spec §4.4 "Resultado y tareas pendientes". */
  const tasks = $derived.by(() => {
    const t: string[] = [];
    if (manifest?.accounts.length) t.push($_("restore.result.accounts", { values: { list: manifest.accounts.map((a) => a.name).join(", ") } }));
    t.push($_("restore.result.bluetooth"), $_("restore.result.lock"));
    if (restoredKeystore.length) t.push($_("restore.result.keystore", { values: { list: restoredKeystore.join(", ") } }));
    if (result?.restored.length) t.push($_("restore.result.notifications"));
    if (result?.other.some((o) => o.item === "contacts" && o.done > 0) && restore.other.contactsAsVcf) t.push($_("restore.result.vcf"));
    if (result?.other.some((o) => o.item === "wallpaper" && o.done > 0)) t.push($_("restore.result.wallpaper"));
    t.push($_("restore.result.reboot"));
    return t;
  });

  let done: Record<number, boolean> = $state({});

  function seconds(ms: number) {
    return ms < 60000 ? `${Math.round(ms / 1000)} s` : `${Math.floor(ms / 60000)} min ${Math.round((ms % 60000) / 1000)} s`;
  }

  let exportError = $state<string | null>(null);

  async function exportList() {
    exportError = null;
    const path = await save({ defaultPath: "adbkeep-pending.txt" });
    if (!path) return;
    const lines = [title, "", `${$_("restore.result.pendingTitle")}:`, ...tasks.map((t, i) => `[${done[i] ? "x" : " "}] ${t}`)];
    if (result?.errors.length) {
      lines.push("", `${$_("backup.result.errors")}:`, ...result.errors.map((e) => `- ${e.package} ${e.step}: ${errorText(e.error)}`));
    }
    const res = await commands.saveText(path, lines.join("\n"));
    if (res.status === "error") exportError = errorText(res.error);
  }
</script>

<section class:ok>
  <h3>{title}</h3>
  {#if error}<p class="error-text">{errorText(error)}</p>{/if}
  {#if result}
    <p>{$_("restore.result.summary", { values: { apps: result.restored.length, time: seconds(result.durationMs) } })}</p>
    {#if result.errors.length}
      <h4>{$_("backup.result.errors")}</h4>
      <ul>
        {#each result.errors as e}
          <li><span class="mono">{e.package} · {e.step}</span>: <span class="error-text">{errorText(e.error)}</span></li>
        {/each}
      </ul>
    {/if}
  {/if}
</section>

<section>
  <h3>{$_("restore.result.pendingTitle")}</h3>
  {#each tasks as t, i}
    <label class="check"><input type="checkbox" bind:checked={done[i]} /> {t}</label>
  {/each}
  <div class="actions">
    <button onclick={exportList}>{$_("restore.result.export")}</button>
    {#if exportError}<p class="error-text">{exportError}</p>{/if}
    <button class="primary" onclick={resetRestore}>{$_("restore.result.newRestore")}</button>
  </div>
</section>

{#if result?.other.length}
  <section>
    <h3>{$_("restore.result.otherTitle")}</h3>
    <ul>
      {#each result.other as o}
        <li>
          {$_(`backup.other.items.${o.item}`)}: {$_("restore.result.otherDone", { values: { done: o.done, failed: o.failed } })}
          {#if settings.advanced && o.errors.length}<div class="mono small error-text">{o.errors.join("; ")}</div>{/if}
        </li>
      {/each}
    </ul>
  </section>
{/if}

{#if settings.advanced && result?.restored.length}
  <section>
    <h3>{$_("restore.result.perApp")}</h3>
    <table>
      <thead>
        <tr><th></th><th>{$_("restore.result.uid")}</th><th>{$_("restore.result.parts")}</th><th>{$_("restore.result.failedPermissions")}</th></tr>
      </thead>
      <tbody>
        {#each result.restored as r}
          <tr>
            <td class="mono">{r.package}</td>
            <td class="mono">{r.uid ?? "—"}</td>
            <td>{r.dataParts.map((p) => $_(`backup.apps.partsShort.${p}`)).join(" · ") || "—"}</td>
            <td class="mono small">{r.failedPermissions.join(", ") || "—"}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </section>
{/if}

<style>
  section {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 0.75rem 1rem;
    margin-block-end: 0.75rem;
  }
  section:first-child {
    border-inline-start: 4px solid var(--warn);
  }
  section.ok {
    border-inline-start-color: var(--ok);
  }
  h3 {
    margin: 0 0 0.4rem;
    font-size: 1rem;
  }
  .check {
    display: block;
    margin-block: 0.25rem;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }
  table {
    width: 100%;
    border-collapse: collapse;
  }
  th,
  td {
    text-align: start;
    padding: 0.3rem 0.5rem;
    border-block-end: 1px solid var(--border);
  }
  .small {
    font-size: 0.85em;
  }
</style>
