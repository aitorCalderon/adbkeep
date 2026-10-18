<script lang="ts">
  import { _ } from "svelte-i18n";
  import { open } from "@tauri-apps/plugin-dialog";
  import { applyLanguage, LANGUAGES, type LanguagePref } from "$lib/i18n";
  import { settings, setSetting, THEMES, type Theme } from "$lib/settings.svelte";
  import { adb, configureAdb } from "$lib/adb.svelte";
  import { errorText } from "$lib/errors";

  let { onClose }: { onClose: () => void } = $props();

  const LANGUAGE_NAMES: Record<string, string> = { es: "Español", en: "English" };

  async function chooseBackupDir() {
    const dir = await open({ directory: true, defaultPath: settings.backupDir ?? undefined });
    if (typeof dir === "string") await setSetting("backupDir", dir);
  }

  async function changeLanguage(lang: LanguagePref) {
    await setSetting("language", lang);
    await applyLanguage(lang);
  }

  async function changeAdbPath(path: string | null) {
    await setSetting("adbPath", path?.trim() ? path.trim() : null);
    await configureAdb();
  }

  async function chooseAdb() {
    const file = await open({ directory: false });
    if (typeof file === "string") await changeAdbPath(file);
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onClose()} />

<div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && onClose()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="prefs-title">
    <h2 id="prefs-title">{$_("prefs.title")}</h2>

    <div class="field">
      <span class="label">{$_("prefs.backupDir")}</span>
      <div class="row">
        <span class="mono value">{settings.backupDir ?? $_("prefs.backupDirUnset")}</span>
        <button onclick={chooseBackupDir}>{$_("prefs.choose")}</button>
      </div>
    </div>

    <label class="field">
      <span class="label">{$_("prefs.language")}</span>
      <select value={settings.language} onchange={(e) => changeLanguage(e.currentTarget.value as LanguagePref)}>
        <option value="system">{$_("prefs.languageSystem")}</option>
        {#each LANGUAGES as lang}
          <option value={lang}>{LANGUAGE_NAMES[lang]}</option>
        {/each}
      </select>
    </label>

    <label class="field check">
      <input type="checkbox" checked={settings.advanced} onchange={(e) => setSetting("advanced", e.currentTarget.checked)} />
      <span>{$_("topbar.advanced")}: <span class="muted">{$_("prefs.advanced")}</span></span>
    </label>

    <label class="field check">
      <input type="checkbox" checked={settings.useHelper} onchange={(e) => setSetting("useHelper", e.currentTarget.checked)} />
      <span>{$_("prefs.useHelper")}<br /><span class="muted">{$_("prefs.useHelperHint")}</span></span>
    </label>

    <label class="field">
      <span class="label">{$_("prefs.theme")}</span>
      <select value={settings.theme} onchange={(e) => setSetting("theme", e.currentTarget.value as Theme)}>
        {#each THEMES as t}
          <option value={t}>{$_("prefs.themes." + t)}</option>
        {/each}
      </select>
    </label>

    <label class="field check">
      <input type="checkbox" checked={settings.systemFont} onchange={(e) => setSetting("systemFont", e.currentTarget.checked)} />
      <span>{$_("prefs.systemFont")}<br /><span class="muted">{$_("prefs.systemFontHint")}</span></span>
    </label>

    {#if settings.advanced}
      <div class="field">
        <span class="label">{$_("prefs.adbPath")}</span>
        <div class="row">
          <input
            type="text"
            class="mono"
            placeholder={$_("prefs.adbPathAuto")}
            value={settings.adbPath ?? ""}
            onchange={(e) => changeAdbPath(e.currentTarget.value)}
          />
          <button onclick={chooseAdb}>{$_("prefs.choose")}</button>
        </div>
        <span class="muted">{$_("prefs.adbPathHint")}</span>
        {#if adb.status}
          <span>
            {$_("prefs.adbStatus")}:
            {#if adb.status.error}
              <span class="error-text">{errorText(adb.status.error)}</span>
            {:else}
              <span class="mono">{adb.status.version}</span> ·
              <span class="mono">{adb.status.path}</span>
              {#if adb.status.source}({$_(`prefs.adbSource.${adb.status.source}`)}){/if}
            {/if}
          </span>
        {/if}
      </div>
    {/if}

    <div class="actions">
      <button class="primary" onclick={onClose}>{$_("prefs.close")}</button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgb(0 0 0 / 0.35);
    display: grid;
    place-items: center;
  }
  .dialog {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 1.25rem 1.5rem;
    width: min(640px, 92vw);
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }
  h2 {
    margin: 0;
    font-size: 1.2rem;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }
  .field.check {
    flex-direction: row;
    align-items: center;
    gap: 0.5rem;
  }
  .label {
    font-weight: 600;
  }
  .row {
    display: flex;
    gap: 0.5rem;
    align-items: center;
  }
  .row input,
  .value {
    flex: 1;
    overflow-wrap: anywhere;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
  }
</style>
