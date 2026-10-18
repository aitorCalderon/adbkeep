<script lang="ts">
  import { _ } from "svelte-i18n";
  import { commands, type WifiView } from "$lib/bindings";
  import { load } from "$lib/viewer";

  let { path }: { path: string } = $props();

  let networks = $state<WifiView[] | null>(null);
  let error = $state<string | null>(null);
  /** Passwords are only fetched from the backup when asked for, and dropped when hidden. */
  let reveal = $state(false);

  $effect(() => {
    const r = reveal;
    load(commands.backupWifi(path, r)).then((res) => {
      networks = res.data;
      error = res.error;
    });
  });
</script>

{#if error}
  <p class="error-text">{error}</p>
{:else if !networks}
  <p class="muted">{$_("library.view.loading")}</p>
{:else if networks.length === 0}
  <p class="muted">{$_("library.view.wifi.empty")}</p>
{:else}
  <div class="view-toolbar">
    <span class="muted">{networks.length}</span>
    <button onclick={() => (reveal = !reveal)}>{reveal ? $_("library.view.wifi.hide") : $_("library.view.wifi.show")}</button>
  </div>
  <div class="view-pane">
    <table class="data-table">
      <thead>
        <tr>
          <th>{$_("library.view.wifi.ssid")}</th>
          <th>{$_("library.view.wifi.security")}</th>
          <th>{$_("library.view.wifi.password")}</th>
        </tr>
      </thead>
      <tbody>
        {#each networks as n}
          <tr>
            <td>{n.ssid}{#if n.hidden}{" "}<span class="muted">({$_("library.view.wifi.hidden")})</span>{/if}</td>
            <td>{$_(`library.view.wifi.securityNames.${n.security}`)}</td>
            <td class="mono">
              {#if n.security === "enterprise"}
                <span class="muted">—</span>
              {:else if !n.hasPassword}
                <span class="muted">{$_("library.view.wifi.none")}</span>
              {:else if n.password !== null}
                <span class="secret">{n.password}</span>
              {:else}
                ••••••••
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{/if}

<style>
  .secret {
    user-select: all;
    overflow-wrap: anywhere;
  }
</style>
