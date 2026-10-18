<script lang="ts">
  import { _ } from "svelte-i18n";
  import { device, displayName } from "$lib/device.svelte";
  import { settings } from "$lib/settings.svelte";
  import { term } from "$lib/glossary";
  import { formatSize } from "$lib/format";

  const snap = $derived(device.snapshot);
  const info = $derived(snap?.info ?? null);
  const ready = $derived(snap?.state.kind === "ready");
  const entry = $derived(snap?.devices.find((d) => d.serial === snap?.selected) ?? null);
  const rootName = $derived(
    snap?.root?.method === "su" ? `su · ${snap.root.suProvider ?? "?"}` : snap?.root?.method === "adbRoot" ? "adb root" : "",
  );
</script>

<button
  class="device-bar"
  class:ready
  onclick={() => (device.panelOpen = !device.panelOpen)}
  title={$_("device.openPanel")}
  aria-expanded={device.panelOpen}
>
  <span class="main">
    📱
    {#if !snap || snap.state.kind === "noDevice" || snap.state.kind === "noAdb"}
      {$_("device.none")}
    {:else if ready && info}
      <strong>{displayName(info)}</strong>
      <span>{$_("device.android", { values: { version: info.androidRelease } })}</span>
      {#if info.battery}
        <span>🔋 {$_("device.battery", { values: { level: info.battery.level } })}{info.battery.plugged ? ` (${$_("device.charging")})` : ""}</span>
      {/if}
      <span>{$_("device.rootOk", { values: { root: $term("root", settings.advanced) } })}</span>
      {#if info.storage}
        <span>{$_("device.free", { values: { size: formatSize(info.storage.freeBytes) } })}</span>
      {/if}
    {:else}
      {#if info || entry}<strong>{displayName(info, entry?.model)}</strong>{/if}
      <span class="state">{$_(`connection.state.${snap.state.kind}`)}</span>
    {/if}
  </span>
  {#if settings.advanced && info}
    <span class="extra mono">
      <span>{$_("device.serial")}: {info.serial}</span>
      {#if rootName}<span>{$_("device.rootMethod")}: {rootName}</span>{/if}
      <span>{$_("device.sdk", { values: { sdk: info.sdk } })}</span>
      <span>{info.abis.join(",")}</span>
      <span>{$_("device.build")}: {info.buildType}</span>
      <span>sqlite3 {info.tools.some((t) => t.name === "sqlite3" && t.path) ? "✔" : "✘"}</span>
    </span>
  {/if}
  <span class="chevron">{device.panelOpen ? "▴" : "▾"}</span>
</button>

<style>
  .device-bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.25rem 1.25rem;
    width: 100%;
    text-align: start;
    padding: 0.4rem 1rem;
    background: var(--surface-2);
    border: none;
    border-block-end: 1px solid var(--border);
    border-radius: 0;
  }
  .main,
  .extra {
    display: flex;
    flex-wrap: wrap;
    gap: 0.25rem 1rem;
    align-items: center;
  }
  .extra {
    color: var(--muted);
  }
  .state {
    color: var(--warn);
  }
  .chevron {
    margin-inline-start: auto;
    color: var(--muted);
  }
</style>
