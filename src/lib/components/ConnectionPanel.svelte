<script lang="ts">
  import { _, json } from "svelte-i18n";
  import { commands, type RootMethod } from "$lib/bindings";
  import { device, displayName } from "$lib/device.svelte";
  import { settings } from "$lib/settings.svelte";
  import { errorText } from "$lib/errors";

  const snap = $derived(device.snapshot);
  const kind = $derived(snap?.state.kind ?? "noDevice");
  const info = $derived(snap?.info ?? null);
  const root = $derived(snap?.root ?? null);
  const busy = $derived(kind === "probing" || kind === "rootPending" || kind === "connecting");
  const canRetry = $derived(kind === "rootDenied" || kind === "noRoot" || kind === "failed" || kind === "locked");

  let adbRootError = $state<string | null>(null);

  async function useAdbRoot() {
    adbRootError = null;
    const res = await commands.enableAdbRoot();
    if (res.status === "error") adbRootError = errorText(res.error);
  }

  function rootLabel(m: RootMethod) {
    return $_(`connection.rootMethod.${m}`, { values: { provider: root?.suProvider ?? "su" } });
  }
</script>

<section class="panel" aria-labelledby="conn-title">
  <h2 id="conn-title">{$_("connection.title")}</h2>

  {#if snap && snap.devices.length > 1}
    <label class="choose">
      {$_("connection.choose")}
      <select value={snap.selected} onchange={(e) => commands.selectDevice(e.currentTarget.value)}>
        {#each snap.devices as d}
          <option value={d.serial}>{displayName(null, d.model) || d.serial} ({d.serial})</option>
        {/each}
      </select>
    </label>
  {/if}

  <div class="status" class:ok={kind === "ready"}>
    {#if busy}<span class="spinner" aria-hidden="true"></span>{/if}
    <h3>{$_(`connection.state.${kind}`)}</h3>
  </div>

  {#if kind === "noDevice"}
    <ol>
      {#each $json("connection.guide.noDevice") as string[] as step}
        <li>{step}</li>
      {/each}
    </ol>
  {:else if kind === "noRoot"}
    <p>{$_("connection.guide.noRoot")}</p>
    <ul>
      <li>{$_("connection.guide.noRootSu")}</li>
      <li>{$_("connection.guide.noRootAdb")}</li>
    </ul>
    {#if info?.adbRootPossible}
      <button onclick={useAdbRoot}>{$_("connection.guide.useAdbRoot")}</button>
      {#if adbRootError}<p class="error-text">{adbRootError}</p>{/if}
    {/if}
  {:else if kind === "failed" && snap?.state.kind === "failed"}
    <p>{$_("connection.guide.failed")}</p>
    <p class="error-text">{errorText(snap.state.error)}</p>
  {:else if kind === "ready" && info}
    <p>
      {$_("connection.summary", { values: { model: displayName(info), version: info.androidRelease } })}
    </p>
  {:else if kind !== "probing"}
    <p>{$_(`connection.guide.${kind}`)}</p>
  {/if}

  <div class="actions">
    {#if canRetry}
      <button onclick={() => commands.retryDevice()}>{$_("connection.retry")}</button>
    {/if}
    {#if kind === "ready"}
      <button class="primary" onclick={() => (device.panelOpen = false)}>{$_("connection.continue")}</button>
    {/if}
  </div>

  {#if settings.advanced && info}
    <div class="advanced">
      {#if root && root.available.length > 1}
        <fieldset>
          <legend>{$_("connection.rootMethods")}</legend>
          {#each root.available as m}
            <label>
              <input type="radio" name="root" checked={root.method === m} onchange={() => commands.setRootMethod(m)} />
              {rootLabel(m)}
            </label>
          {/each}
        </fieldset>
      {/if}
      <div>
        <h4>{$_("connection.tools")}</h4>
        <ul class="tools mono">
          {#each info.tools as t}
            <li class:missing={!t.path} title={t.path ?? ""}>{t.path ? "✔" : "✘"} {t.name}</li>
          {/each}
        </ul>
      </div>
      <div>
        <h4>{$_("connection.users")}</h4>
        <ul class="mono">
          {#each info.users as u}
            <li>{u.id} · {u.name}{u.running ? ` · ${$_("connection.userRunning")}` : ""}</li>
          {/each}
        </ul>
      </div>
    </div>
  {/if}
</section>

<style>
  .panel {
    max-width: 720px;
    margin: 1.5rem auto;
    padding: 1.25rem 1.5rem;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 10px;
  }
  h2 {
    margin: 0 0 1rem;
    font-size: 1.1rem;
    color: var(--muted);
    font-weight: 500;
  }
  h3 {
    margin: 0;
    font-size: 1.3rem;
  }
  h4 {
    margin: 0 0 0.3rem;
  }
  .status {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    margin-block-end: 0.75rem;
  }
  .status.ok h3 {
    color: var(--ok);
  }
  .choose {
    display: flex;
    gap: 0.5rem;
    align-items: center;
    margin-block-end: 1rem;
  }
  ol li,
  ul li {
    margin-block: 0.3rem;
  }
  .actions {
    display: flex;
    gap: 0.5rem;
    justify-content: flex-end;
    margin-block-start: 1rem;
  }
  .advanced {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
    gap: 1rem;
    margin-block-start: 1.25rem;
    padding-block-start: 1rem;
    border-block-start: 1px solid var(--border);
  }
  .advanced ul {
    list-style: none;
    padding: 0;
    margin: 0;
  }
  .missing {
    color: var(--error);
  }
  fieldset {
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }
  .spinner {
    width: 1rem;
    height: 1rem;
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
</style>
