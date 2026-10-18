<script lang="ts">
  import { _ } from "svelte-i18n";
  import { confirm } from "@tauri-apps/plugin-dialog";
  import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
  import { commands, type BackupListing, type FileEntry, type Problem } from "$lib/bindings";
  import { library, refreshLibrary, runAction, verify } from "$lib/library.svelte";
  import { device } from "$lib/device.svelte";
  import { prepareResume } from "$lib/backup.svelte";
  import UnlockForm from "./UnlockForm.svelte";
  import { settings } from "$lib/settings.svelte";
  import { formatDate, formatSize } from "$lib/format";
  import { errorText } from "$lib/errors";
  import { nav } from "$lib/nav.svelte";
  import { loadPreview, restore, type RestoreOnly } from "$lib/restore.svelte";
  import { pickFile } from "$lib/viewer";
  import type { OtherItem } from "$lib/bindings";
  import MessagesView from "./MessagesView.svelte";
  import ContactsView from "./ContactsView.svelte";
  import CallsView from "./CallsView.svelte";
  import SharedFilesView from "./SharedFilesView.svelte";
  import WifiNetworksView from "./WifiNetworksView.svelte";
  import SettingsView from "./SettingsView.svelte";
  import ArchiveBrowser from "./ArchiveBrowser.svelte";

  let { backup }: { backup: BackupListing } = $props();

  type Tab = "summary" | "user" | "system" | "messages" | "contacts" | "calls" | "shared" | "wifi" | "warnings" | "files" | "settings" | "log";
  let tab: Tab = $state("summary");
  let viewing: { path: string; text: string | null; archive: boolean } | null = $state(null);
  let saveStatus: { text: string; bad: boolean } | null = $state(null);
  let logText: string | null = $state(null);

  const m = $derived(backup.manifest);
  const usable = $derived(backup.status.kind === "complete" || backup.status.kind === "incomplete");
  const userApps = $derived(m?.apps.filter((a) => a.kind === "user") ?? []);
  const systemApps = $derived(m?.apps.filter((a) => a.kind === "updatedSystem") ?? []);
  const progress = $derived(library.verifying[backup.path]);
  const report = $derived(library.reports[backup.path]);
  const busy = $derived(library.busy === backup.path);
  /** Items of the Resto tab that the backup holds files for. */
  const has = (item: OtherItem) => !!m?.other?.some((e) => e.item === item && e.files.length > 0 && e.count !== 0);
  const tabs = $derived.by<Tab[]>(() => {
    const t: Tab[] = ["summary", "user", "system"];
    if (has("sms")) t.push("messages");
    if (has("contacts")) t.push("contacts");
    if (has("calls")) t.push("calls");
    if (has("files")) t.push("shared");
    if (has("wifi")) t.push("wifi");
    t.push("warnings");
    if (settings.advanced) {
      t.push("files");
      if (has("settings")) t.push("settings");
      t.push("log");
    }
    return t;
  });
  /** Resto items with what they hold, for the summary. */
  const otherSummary = $derived(
    (m?.other ?? [])
      .filter((e) => e.files.length > 0)
      .map((e) => ({
        item: e.item,
        text: $_(`backup.other.items.${e.item}`) + (e.count !== null && e.item !== "files" && e.item !== "settings" ? ` (${e.count})` : ""),
      })),
  );
  /** Resuming needs the phone the backup was made from, ready. */
  const canResume = $derived(
    backup.resumable && !!m && device.snapshot?.selected === m.source.serial && device.snapshot?.state.kind === "ready",
  );
  type PackForm = { encrypt: boolean; password: string; repeat: string };
  let packing = $state<PackForm | null>(null);
  const packOk = $derived.by(() => {
    const p = packing;
    return !!p && (!p.encrypt || (p.password.length > 0 && p.password === p.repeat));
  });

  const TEXT = /\.(json|jsonl|log|txt|xml|vcf)$/;

  async function view(f: FileEntry) {
    saveStatus = null;
    if (f.path.endsWith(".tar.zst")) {
      viewing = { path: f.path, text: null, archive: true };
      return;
    }
    if (!TEXT.test(f.path)) {
      viewing = { path: f.path, text: null, archive: false };
      return;
    }
    const res = await commands.readBackupFile(backup.path, f.path);
    viewing = { path: f.path, text: res.status === "ok" ? res.data : errorText(res.error), archive: false };
  }

  async function saveViewed() {
    if (!viewing) return;
    const name = viewing.path.split("/").pop() ?? "file";
    const dest = await pickFile(name, name.split(".").pop() ?? "");
    if (!dest) return;
    const r = await commands.exportBackupFile(backup.path, viewing.path, dest);
    saveStatus = r.status === "ok" ? { text: $_("library.view.saved", { values: { path: dest } }), bad: false } : { text: errorText(r.error), bad: true };
  }

  $effect(() => {
    if (tab === "log" && logText === null) {
      commands.readBackupFile(backup.path, "report/session.log").then((r) => (logText = r.status === "ok" ? r.data : ""));
    }
  });

  async function remove() {
    const ok = await confirm($_("library.actions.confirmDelete", { values: { name: backup.name } }), { kind: "warning" });
    if (ok) await runAction(backup.path, () => commands.deleteBackup(backup.path), () => null);
  }

  function problemText(p: Problem) {
    return $_(`library.verify.problem.${p.kind}`);
  }

  async function openRestore(only: RestoreOnly | null = null) {
    nav.section = "restore";
    restore.tab = "user";
    await loadPreview(backup.path, only);
  }

  async function resume() {
    const e = await prepareResume(backup.path, backup.name);
    if (e) library.error = e;
    else nav.section = "backup";
  }

  async function pack() {
    if (!packing) return;
    const password = packing.encrypt ? packing.password : null;
    packing = null;
    await runAction(backup.path, () => commands.packBackup(backup.path, password), (p) => p);
  }

  async function openFolder() {
    if (backup.packed) await revealItemInDir(backup.path);
    else await openPath(backup.path);
  }
</script>

<header>
  <h2>{backup.name}</h2>
  <div class="actions">
    <button disabled={!usable || busy} onclick={() => openRestore()}>{$_("library.actions.restore")}</button>
    <button disabled={!usable || !!progress || busy} onclick={() => verify(backup.path)}>
      {progress ? $_("library.actions.verifying", { values: progress }) : $_("library.actions.verify")}
    </button>
    {#if usable}
      {#if backup.packed}
        <button disabled={busy || !!progress} onclick={() => runAction(backup.path, () => commands.unpackBackup(backup.path), (p) => p)}>
          {busy ? $_("library.actions.working") : $_("library.actions.unpack")}
        </button>
      {:else if backup.status.kind === "complete"}
        <button disabled={busy || !!progress || !!packing} onclick={() => (packing = { encrypt: false, password: "", repeat: "" })}>
          {busy ? $_("library.actions.working") : $_("library.actions.pack")}
        </button>
      {/if}
    {/if}
    {#if backup.resumable}
      <button
        disabled={!canResume || busy}
        title={canResume ? "" : $_("library.actions.resumeHelp", { values: { model: m?.source.model ?? "" } })}
        onclick={resume}>{$_("library.actions.resume")}</button>
    {/if}
    <button onclick={openFolder}>{$_("library.actions.openFolder")}</button>
    <button class="danger" disabled={busy || !!progress} onclick={remove}>{$_("library.actions.delete")}</button>
  </div>
</header>

{#if packing}
  <form class="pack-form" onsubmit={(e) => { e.preventDefault(); pack(); }}>
    <strong>{$_("library.pack.title")}</strong>
    <label><input type="checkbox" bind:checked={packing.encrypt} /> {$_("library.pack.encrypt")}</label>
    {#if packing.encrypt}
      <input type="password" autocomplete="new-password" placeholder={$_("backup.action.password")} aria-label={$_("backup.action.password")} bind:value={packing.password} />
      <input type="password" autocomplete="new-password" placeholder={$_("backup.action.passwordRepeat")} aria-label={$_("backup.action.passwordRepeat")} bind:value={packing.repeat} />
      {#if packing.repeat && packing.password !== packing.repeat}
        <span class="error-text">{$_("backup.action.passwordMismatch")}</span>
      {:else}
        <span class="muted">{$_("backup.action.passwordHelp")}</span>
      {/if}
    {/if}
    <span class="spacer"></span>
    <button type="button" onclick={() => (packing = null)}>{$_("library.pack.cancel")}</button>
    <button type="submit" class="primary" disabled={!packOk}>{$_("library.pack.start")}</button>
  </form>
{/if}

{#if backup.status.kind !== "complete"}
  <div class="notice">
    {#if backup.status.kind === "incomplete" && backup.resumable}
      {$_("library.statusHelp.incompleteResumable")}
    {:else}
      {$_(`library.statusHelp.${backup.status.kind}`)}
    {/if}
    {#if backup.status.kind === "unreadable"}<span class="error-text">{errorText(backup.status.error)}</span>{/if}
    {#if backup.status.kind === "encrypted"}
      <UnlockForm path={backup.path} onUnlocked={refreshLibrary} />
    {/if}
  </div>
{/if}

{#if report}
  <div class="report" class:ok={report.problems.length === 0}>
    {#if report.problems.length === 0}
      ✔ {$_("library.verify.ok", { values: { count: report.filesChecked } })}
    {:else}
      ✘ {$_("library.verify.bad", { values: { count: report.problems.length } })}
      <ul>
        {#each report.problems as p}
          <li><span class="mono">{p.path}</span>: {problemText(p.problem)}</li>
        {/each}
      </ul>
    {/if}
  </div>
{/if}

{#if m}
  <div class="tabs" role="tablist">
    {#each tabs as t}
      <button role="tab" aria-selected={tab === t} class:active={tab === t} onclick={() => (tab = t)}>
        {$_(`library.tabs.${t}`)}
        {#if t === "warnings" && m.keystorePackages.length + m.accounts.length + m.errors.length > 0}
          <span class="count">{m.keystorePackages.length + m.accounts.length + m.errors.length}</span>
        {/if}
      </button>
    {/each}
  </div>

  {#if tab === "summary"}
    <dl>
      <dt>{$_("library.summary.source")}</dt>
      <dd>{m.source.manufacturer} {m.source.model}{#if settings.advanced}{" "}<span class="mono muted">({m.source.serial} · {m.source.buildId} · {m.source.buildType})</span>{/if}</dd>
      <dt>Android</dt>
      <dd>{$_("library.summary.android", { values: { version: m.source.androidRelease, sdk: m.source.sdk } })}</dd>
      <dt>{$_("library.summary.date")}</dt>
      <dd>{formatDate(m.createdAt)}</dd>
      <dt>{$_("library.summary.size")}</dt>
      <dd>{formatSize(backup.size)}</dd>
      <dt>{$_("library.summary.contents")}</dt>
      <dd>
        {$_("library.summary.apps", { values: { apps: m.apps.length, withData: m.apps.filter((a) => a.data.length > 0).length } })}
        {#if otherSummary.length}
          <br />{$_("library.summary.other")}:
          {#each otherSummary as o, i}
            {#if i > 0}{" · "}{/if}{o.text}{#if usable && o.item !== "notifications"}{" "}<button
                class="link small"
                title={$_("library.actions.restoreItemHelp")}
                onclick={() => openRestore({ kind: "other", item: o.item })}>{$_("library.actions.restoreItem")}</button>{/if}
          {/each}
        {/if}
        {#if m.errors.length}<br /><span class="warn-text">{$_("library.summary.errors", { values: { count: m.errors.length } })}</span>{/if}
      </dd>
      <dt>{$_("library.summary.location")}</dt>
      <dd class="mono">{backup.path}</dd>
    </dl>
  {:else if tab === "user" || tab === "system"}
    {@const apps = tab === "user" ? userApps : systemApps}
    {#if apps.length === 0}
      <p class="muted">{$_("library.apps.none")}</p>
    {:else}
      <table>
        <thead>
          <tr>
            <th></th>
            <th>{$_("library.apps.version")}</th>
            <th>{$_("library.apps.data")}</th>
            <th>{$_("library.apps.permissions")}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each apps as a}
            <tr>
              <td>
                {a.label || a.package}{#if m.keystorePackages.includes(a.package)} ⚠️{/if}
                {#if a.label && settings.advanced}<div class="mono muted">{a.package}</div>{/if}
              </td>
              <td class="mono">{a.versionName ?? ""}{#if settings.advanced}{" "}({a.versionCode}){/if}</td>
              <td>
                {#if a.data.length === 0}
                  <span class="muted">{$_("library.apps.apkOnly")}</span>
                {:else if settings.advanced}
                  {a.data.map((d) => $_(`backup.apps.partsShort.${d.part}`)).join(" · ")}
                {:else}✔{/if}
              </td>
              <td>{a.permissions ? "✔" : ""}</td>
              <td>
                {#if usable}
                  <button class="link small" title={$_("library.actions.restoreItemHelp")} onclick={() => openRestore({ kind: "app", package: a.package })}>
                    {$_("library.actions.restoreItem")}
                  </button>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  {:else if tab === "warnings"}
    {#if m.keystorePackages.length}
      <h3>⚠️ {$_("backup.warnings.keystoreTitle")}</h3>
      <p>{$_("backup.warnings.keystoreText")}</p>
      <ul>{#each m.keystorePackages as p}<li class="mono">{p}</li>{/each}</ul>
    {/if}
    <h3>👤 {$_("backup.warnings.accountsTitle")}</h3>
    {#if m.accounts.length}
      <p>{$_("backup.warnings.accountsText")}</p>
      <ul>{#each m.accounts as acc}<li>{acc.name} <span class="muted mono">({acc.accountType})</span></li>{/each}</ul>
    {:else}
      <p class="muted">{$_("backup.warnings.accountsNone")}</p>
    {/if}
    {#if m.errors.length}
      <h3>✘ {$_("backup.result.errors")}</h3>
      <ul>
        {#each m.errors as e}
          <li><span class="mono">{e.package} · {e.step}</span>: <span class="error-text">{errorText(e.error)}</span></li>
        {/each}
      </ul>
    {/if}
  {:else if tab === "files"}
    <div class="files">
      <table>
        <thead><tr><th>{$_("library.files.path")}</th><th class="num">{$_("library.files.size")}</th><th>SHA-256</th></tr></thead>
        <tbody>
          {#each m.files as f}
            <tr class:selected={viewing?.path === f.path}>
              <td><button class="link mono" onclick={() => view(f)}>{f.path}</button></td>
              <td class="num">{formatSize(f.size)}</td>
              <td class="mono muted" title={f.sha256}>{f.sha256.slice(0, 12)}…</td>
            </tr>
          {/each}
        </tbody>
      </table>
      <div class="viewer">
        {#if !viewing}
          <p class="muted">{$_("library.files.choose")}</p>
        {:else}
          <div class="viewer-head">
            <span class="mono">{viewing.path}</span>
            <button onclick={saveViewed}>{$_("library.view.raw.save")}</button>
          </div>
          {#if saveStatus}<p class="view-status" class:bad={saveStatus.bad}>{saveStatus.text}</p>{/if}
          {#if viewing.archive}
            <p class="muted">{$_("library.view.raw.archive")}</p>
            {#key viewing.path}<ArchiveBrowser path={backup.path} rel={viewing.path} />{/key}
          {:else if viewing.text === null}
            <p class="muted">{$_("library.files.binary")}</p>
          {:else}
            <pre class="mono">{viewing.text}</pre>
          {/if}
        {/if}
      </div>
    </div>
  {:else if tab === "messages"}
    <MessagesView path={backup.path} />
  {:else if tab === "contacts"}
    <ContactsView path={backup.path} />
  {:else if tab === "calls"}
    <CallsView path={backup.path} />
  {:else if tab === "shared"}
    <SharedFilesView path={backup.path} manifest={m} />
  {:else if tab === "wifi"}
    <WifiNetworksView path={backup.path} />
  {:else if tab === "settings"}
    <SettingsView path={backup.path} />
  {:else if tab === "log"}
    {#if logText}
      <pre class="mono log">{logText}</pre>
    {:else if logText === ""}
      <p class="muted">{$_("library.logEmpty")}</p>
    {/if}
  {/if}
{/if}

<style>
  header {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem 1rem;
    margin-block-end: 0.5rem;
  }
  h2 {
    margin: 0;
    font-size: 1.15rem;
    overflow-wrap: anywhere;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
    margin-inline-start: auto;
  }
  .danger {
    color: var(--error);
  }
  .pack-form {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.4rem 0.75rem;
    padding: 0.5rem 0.75rem;
    margin-block-end: 0.5rem;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }
  .pack-form input[type="password"] {
    width: 11rem;
  }
  .spacer {
    flex: 1;
  }
  .small {
    font-size: 0.85em;
  }
  .notice {
    border-inline-start: 4px solid var(--warn);
    padding: 0.4rem 0.75rem;
    background: var(--surface);
  }
  .report {
    border-inline-start: 4px solid var(--error);
    padding: 0.4rem 0.75rem;
    background: var(--surface);
    margin-block: 0.5rem;
  }
  .report.ok {
    border-inline-start-color: var(--ok);
  }
  .tabs {
    display: flex;
    flex-wrap: wrap;
    gap: 0.25rem;
    border-block-end: 1px solid var(--border);
    margin-block: 0.75rem;
  }
  .tabs button {
    border: 1px solid transparent;
    border-block-end: none;
    border-radius: var(--radius) var(--radius) 0 0;
    background: transparent;
  }
  .tabs button.active {
    background: var(--surface);
    border-color: var(--border);
    font-weight: 600;
    margin-block-end: -1px;
  }
  .count {
    font-size: 0.8em;
    color: var(--warn);
    margin-inline-start: 0.2em;
  }
  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 0.4rem 1.25rem;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    background: var(--surface);
  }
  th,
  td {
    text-align: start;
    padding: 0.3rem 0.5rem;
    border-block-end: 1px solid var(--border);
  }
  th {
    color: var(--muted);
    font-weight: 600;
    font-size: 0.9em;
  }
  .num {
    text-align: end;
    white-space: nowrap;
  }
  h3 {
    font-size: 1rem;
    margin: 0.75rem 0 0.3rem;
  }
  .files {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: 0.75rem;
    align-items: start;
  }
  tr.selected td {
    background: var(--surface-2);
  }
  .link {
    border: none;
    background: none;
    color: var(--accent);
    padding: 0;
    text-align: start;
    overflow-wrap: anywhere;
  }
  .viewer-head {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    align-items: center;
    justify-content: space-between;
    margin-block-end: 0.4rem;
    overflow-wrap: anywhere;
  }
  .viewer pre,
  .log {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 0.5rem;
    white-space: pre-wrap;
    word-break: break-all;
    max-height: 60vh;
    overflow: auto;
    margin: 0;
  }
  .warn-text {
    color: var(--warn);
  }
</style>
