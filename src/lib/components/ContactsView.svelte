<script lang="ts">
  import { _ } from "svelte-i18n";
  import { commands, type Contact, type ContactField } from "$lib/bindings";
  import { settings } from "$lib/settings.svelte";
  import { fold, load, pickFile } from "$lib/viewer";
  import { errorText } from "$lib/errors";

  let { path }: { path: string } = $props();

  let contacts = $state<Contact[] | null>(null);
  let error = $state<string | null>(null);
  let query = $state("");
  let current = $state<number | null>(null);
  let status = $state<{ text: string; bad: boolean } | null>(null);

  $effect(() => {
    load(commands.backupContacts(path)).then((r) => {
      contacts = r.data;
      error = r.error;
    });
  });

  const name = (c: Contact) => c.name || $_("library.view.contacts.noName");
  const q = $derived(fold(query.trim()));
  const visible = $derived(
    (contacts ?? []).filter((c) => !q || fold(c.name).includes(q) || c.fields.some((f) => fold(f.value).includes(q))),
  );
  const open = $derived(contacts?.find((c) => c.id === current) ?? null);

  function typeLabel(f: ContactField): string {
    if (f.label) return f.label;
    if (f.typeCode === null) return "";
    const key = `library.view.contacts.types.${f.kind}.${f.typeCode}`;
    const t = $_(key);
    return t === key ? "" : t;
  }

  async function exportVcf() {
    const dest = await pickFile("contactos.vcf", "vcf");
    if (!dest) return;
    const r = await commands.exportVcf(path, dest);
    status =
      r.status === "ok"
        ? { text: $_("library.view.contacts.exported", { values: { count: r.data, path: dest } }), bad: false }
        : { text: errorText(r.error), bad: true };
  }
</script>

{#if error}
  <p class="error-text">{error}</p>
{:else if !contacts}
  <p class="muted">{$_("library.view.loading")}</p>
{:else if contacts.length === 0}
  <p class="muted">{$_("library.view.contacts.empty")}</p>
{:else}
  <div class="view-toolbar">
    <input type="search" placeholder={$_("library.view.search")} bind:value={query} />
    <span class="muted">{contacts.length}</span>
    <button onclick={exportVcf}>{$_("library.view.contacts.exportVcf")}</button>
  </div>
  {#if status}<p class="view-status" class:bad={status.bad}>{status.text}</p>{/if}
  <div class="view-split">
    <div class="view-pane">
      {#if visible.length === 0}<p class="muted pad">{$_("library.view.noResults")}</p>{/if}
      <ul class="view-list">
        {#each visible as c (c.id)}
          <li>
            <button class:active={current === c.id} onclick={() => (current = c.id)}>
              {#if c.starred}★ {/if}{name(c)}
            </button>
          </li>
        {/each}
      </ul>
    </div>
    <div class="view-pane detail">
      {#if !open}
        <p class="muted">{$_("library.view.contacts.choose")}</p>
      {:else}
        <div class="head">
          {#if open.photo}
            <img src={`data:image/jpeg;base64,${open.photo}`} alt="" />
          {:else}
            <div class="avatar">{(open.name || "?").slice(0, 1).toUpperCase()}</div>
          {/if}
          <div>
            <h3>{name(open)}{#if open.starred}{" "}<span title={$_("library.view.contacts.starred")}>★</span>{/if}</h3>
            <div class="muted small">
              {open.accounts.map((a) => a || $_("library.view.contacts.phoneAccount")).join(" · ")}
            </div>
          </div>
        </div>
        <dl>
          {#each open.fields as f}
            <dt>{$_(`library.view.contacts.kinds.${f.kind}`)}{#if typeLabel(f)}{" "}<span class="muted small">({typeLabel(f)})</span>{/if}</dt>
            <dd>{f.value}</dd>
          {/each}
        </dl>
        {#if settings.advanced}<p class="muted small mono">contact_id {open.id}</p>{/if}
      {/if}
    </div>
  </div>
{/if}

<style>
  .pad {
    padding: 0.5rem;
  }
  .small {
    font-size: 0.85em;
  }
  .detail {
    padding: 0.75rem;
  }
  .head {
    display: flex;
    gap: 0.75rem;
    align-items: center;
    margin-block-end: 0.75rem;
  }
  .head img,
  .avatar {
    width: 3.5rem;
    height: 3.5rem;
    border-radius: 50%;
    object-fit: cover;
  }
  .avatar {
    display: grid;
    place-items: center;
    background: var(--surface-2);
    font-size: 1.5rem;
    font-weight: 600;
  }
  h3 {
    margin: 0;
    font-size: 1.1rem;
  }
  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 0.35rem 1rem;
    margin: 0;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
</style>
