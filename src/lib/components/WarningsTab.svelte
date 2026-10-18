<script lang="ts">
  import { _ } from "svelte-i18n";
  import type { Account, AppInfo } from "$lib/bindings";
  import { settings } from "$lib/settings.svelte";

  let { apps, accounts, onShowApp }: { apps: AppInfo[]; accounts: Account[]; onShowApp: (pkg: string) => void } = $props();

  const keystoreApps = $derived(apps.filter((a) => a.keystore));
  const NOT_COPIED = ["accounts", "bluetooth", "lock", "drm", "imei"];
</script>

<section>
  <h3>⚠️ {$_("backup.warnings.keystoreTitle")}</h3>
  {#if keystoreApps.length}
    <p>{$_("backup.warnings.keystoreText")}</p>
    <ul>
      {#each keystoreApps as app}
        <li>
          <button class="link" onclick={() => onShowApp(app.package)}>{app.label || app.package}</button>
          {#if settings.advanced}<span class="muted mono">UID {app.uid}</span>{/if}
        </li>
      {/each}
    </ul>
  {:else}
    <p class="muted">{$_("backup.warnings.keystoreNone")}</p>
  {/if}
</section>

<section>
  <h3>👤 {$_("backup.warnings.accountsTitle")}</h3>
  {#if accounts.length}
    <p>{$_("backup.warnings.accountsText")}</p>
    <ul>
      {#each accounts as acc}
        <li>{acc.name}{#if settings.advanced}{" "}<span class="muted mono">({acc.accountType})</span>{/if}</li>
      {/each}
    </ul>
  {:else}
    <p class="muted">{$_("backup.warnings.accountsNone")}</p>
  {/if}
</section>

<section>
  <h3>⛔ {$_("backup.warnings.notCopiedTitle")}</h3>
  <ul>
    {#each NOT_COPIED as key}
      <li>{$_(`backup.warnings.notCopied.${key}`)}</li>
    {/each}
  </ul>
</section>

<style>
  section {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 0.75rem 1rem;
    margin-block-end: 0.75rem;
  }
  h3 {
    margin: 0 0 0.4rem;
    font-size: 1rem;
  }
  p {
    margin: 0.3rem 0;
  }
  ul {
    margin: 0.3rem 0;
  }
  .link {
    border: none;
    background: none;
    color: var(--accent);
    padding: 0;
  }
</style>
