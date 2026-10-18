<script lang="ts">
  import { onMount } from "svelte";
  import { _ } from "svelte-i18n";
  import TopBar from "$lib/components/TopBar.svelte";
  import DeviceBar from "$lib/components/DeviceBar.svelte";
  import LogPanel from "$lib/components/LogPanel.svelte";
  import Preferences from "$lib/components/Preferences.svelte";
  import ConnectionPanel from "$lib/components/ConnectionPanel.svelte";
  import { device, isReady, startDevices } from "$lib/device.svelte";
  import { settings } from "$lib/settings.svelte";
  import { configureAdb } from "$lib/adb.svelte";
  import { startLog } from "$lib/log.svelte";
  import BackupSection from "$lib/components/BackupSection.svelte";
  import LibrarySection from "$lib/components/LibrarySection.svelte";
  import RestoreSection from "$lib/components/RestoreSection.svelte";
  import { nav } from "$lib/nav.svelte";
  import { startBackupEvents } from "$lib/backup.svelte";
  import { job } from "$lib/job.svelte";

  // The active section lives in a store so other sections can switch to it (Mis copias → Restaurar).
  const section = $derived(nav.section);
  let prefsOpen = $state(false);

  onMount(async () => {
    await startLog();
    await startDevices();
    await startBackupEvents();
    await configureAdb();
  });
</script>

<div class="window">
  <TopBar bind:section={nav.section} onPreferences={() => (prefsOpen = true)} />
  <DeviceBar />
  <main>
    <!-- Backup and restore need a ready phone; until then the connection panel replaces them (ui-spec §2). -->
    {#if device.panelOpen || (section !== "library" && !isReady() && !job.running)}
      <ConnectionPanel />
    {:else if section === "backup"}
      <BackupSection />
    {:else if section === "library"}
      <LibrarySection />
    {:else if section === "restore"}
      <RestoreSection />
    {:else}
      <p class="muted">{$_("section.comingSoon")}</p>
    {/if}
  </main>
  {#if settings.advanced}
    <LogPanel />
  {/if}
</div>

{#if prefsOpen}
  <Preferences onClose={() => (prefsOpen = false)} />
{/if}

<style>
  .window {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }
  main {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
</style>
