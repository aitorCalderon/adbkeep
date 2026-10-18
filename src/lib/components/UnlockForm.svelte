<script lang="ts">
  import { _ } from "svelte-i18n";
  import { commands, type CoreError } from "$lib/bindings";
  import { errorText } from "$lib/errors";

  /** Asks for the password of an encrypted pack; it is kept in memory for this session only. */
  let { path, onUnlocked }: { path: string; onUnlocked: () => void } = $props();

  let password = $state("");
  let error: CoreError | null = $state(null);
  let working = $state(false);

  async function unlock(e: SubmitEvent) {
    e.preventDefault();
    working = true;
    const res = await commands.unlockBackup(path, password);
    working = false;
    if (res.status === "error") {
      error = res.error;
      return;
    }
    password = "";
    error = null;
    onUnlocked();
  }
</script>

<form onsubmit={unlock}>
  <span>🔒</span>
  <input type="password" autocomplete="current-password" placeholder={$_("backup.action.password")} aria-label={$_("backup.action.password")} bind:value={password} />
  <button type="submit" class="primary" disabled={!password || working}>
    {working ? $_("library.actions.working") : $_("library.unlock")}
  </button>
  {#if error}<span class="error-text">{errorText(error)}</span>{/if}
</form>

<style>
  form {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.4rem;
    margin-block: 0.5rem;
  }
  input {
    width: 14rem;
  }
</style>
