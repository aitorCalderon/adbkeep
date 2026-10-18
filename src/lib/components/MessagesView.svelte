<script lang="ts">
  import { _ } from "svelte-i18n";
  import { commands, type Attachment, type Conversation } from "$lib/bindings";
  import { formatMs } from "$lib/format";
  import { fold, load, pickFile } from "$lib/viewer";
  import { errorText } from "$lib/errors";

  let { path }: { path: string } = $props();

  let conversations = $state<Conversation[] | null>(null);
  let error = $state<string | null>(null);
  let query = $state("");
  let current = $state<number | null>(null);
  let status = $state<{ text: string; bad: boolean } | null>(null);
  /** Part id → data URL of loaded images. */
  let images = $state<Record<number, string>>({});

  $effect(() => {
    load(commands.backupMessages(path)).then((r) => {
      conversations = r.data;
      error = r.error;
    });
  });

  const title = (c: Conversation) =>
    c.addresses.map((a, i) => c.names[i] ?? a).join(", ") || $_("library.view.messages.unknown");

  const q = $derived(fold(query.trim()));
  const visible = $derived(
    (conversations ?? []).filter(
      (c) => !q || fold(title(c) + " " + c.addresses.join(" ")).includes(q) || c.messages.some((m) => fold(m.body).includes(q)),
    ),
  );
  const open = $derived(conversations?.find((c) => c.thread === current) ?? null);
  const total = $derived((conversations ?? []).reduce((n, c) => n + c.messages.length, 0));

  async function exportXml() {
    const dest = await pickFile("sms.xml", "xml");
    if (!dest) return;
    const r = await commands.exportSmsXml(path, dest);
    status =
      r.status === "ok"
        ? { text: $_("library.view.messages.exported", { values: { count: r.data, path: dest } }), bad: false }
        : { text: errorText(r.error), bad: true };
  }

  async function show(a: Attachment) {
    const r = await commands.backupMmsPart(path, a.part);
    if (r.status === "ok") images[a.part] = `data:${r.data.mime};base64,${r.data.base64}`;
    else status = { text: errorText(r.error), bad: true };
  }

  async function saveAttachment(a: Attachment) {
    const ext = a.name?.split(".").pop() ?? a.mime.split("/").pop() ?? "bin";
    const dest = await pickFile(a.name ?? `mms-${a.part}.${ext}`, ext);
    if (!dest) return;
    const r = await commands.exportMmsPart(path, a.part, dest);
    status = r.status === "ok" ? { text: $_("library.view.saved", { values: { path: dest } }), bad: false } : { text: errorText(r.error), bad: true };
  }

  function marked(body: string): { text: string; hit: boolean }[] {
    if (!q) return [{ text: body, hit: false }];
    // Folding keeps lengths for most text; fall back to no highlight when it does not.
    const folded = fold(body);
    if (folded.length !== body.length) return [{ text: body, hit: false }];
    const out: { text: string; hit: boolean }[] = [];
    let i = 0;
    for (let j = folded.indexOf(q); j >= 0; j = folded.indexOf(q, j + q.length)) {
      out.push({ text: body.slice(i, j), hit: false }, { text: body.slice(j, j + q.length), hit: true });
      i = j + q.length;
    }
    out.push({ text: body.slice(i), hit: false });
    return out;
  }
</script>

{#if error}
  <p class="error-text">{error}</p>
{:else if !conversations}
  <p class="muted">{$_("library.view.loading")}</p>
{:else if conversations.length === 0}
  <p class="muted">{$_("library.view.messages.empty")}</p>
{:else}
  <div class="view-toolbar">
    <input type="search" placeholder={$_("library.view.search")} bind:value={query} />
    <span class="muted">{$_("library.view.messages.count", { values: { count: total } })}</span>
    <button onclick={exportXml} title={$_("library.view.messages.xmlHelp")}>{$_("library.view.messages.exportXml")}</button>
  </div>
  {#if status}<p class="view-status" class:bad={status.bad}>{status.text}</p>{/if}
  <div class="view-split">
    <div class="view-pane">
      {#if visible.length === 0}
        <p class="muted pad">{$_("library.view.noResults")}</p>
      {/if}
      <ul class="view-list">
        {#each visible as c (c.thread)}
          {@const last = c.messages[c.messages.length - 1]}
          <li>
            <button class:active={current === c.thread} onclick={() => (current = c.thread)}>
              <strong>{title(c)}</strong> <span class="muted small">· {c.messages.length}</span>
              <div class="muted small ellipsis">{last?.body || (last?.attachments.length ? $_("library.view.messages.attachment") : "")}</div>
              <div class="muted small">{formatMs(c.lastDate)}</div>
            </button>
          </li>
        {/each}
      </ul>
    </div>
    <div class="view-pane thread">
      {#if !open}
        <p class="muted pad">{$_("library.view.messages.choose")}</p>
      {:else}
        <h3>{title(open)}</h3>
        {#each open.messages as m}
          <div class="bubble" class:out={m.outgoing} class:hit={q && fold(m.body).includes(q)}>
            {#if m.from && open.addresses.length > 1}<div class="muted small">{m.from}</div>{/if}
            {#if m.body}<div class="body">{#each marked(m.body) as part}{#if part.hit}<mark>{part.text}</mark>{:else}{part.text}{/if}{/each}</div>{/if}
            {#each m.attachments as a}
              <div class="attachment">
                {#if images[a.part]}
                  <img src={images[a.part]} alt={a.name ?? a.mime} />
                {:else}
                  📎 {a.name ?? a.mime}
                  {#if a.mime.startsWith("image/")}<button class="small-btn" onclick={() => show(a)}>{$_("library.view.messages.show")}</button>{/if}
                {/if}
                <button class="small-btn" onclick={() => saveAttachment(a)}>{$_("library.view.messages.save")}</button>
              </div>
            {/each}
            <div class="muted small time">{formatMs(m.date)}{m.mms ? " · MMS" : ""}</div>
          </div>
        {/each}
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
  .ellipsis {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .thread {
    padding: 0.5rem;
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }
  h3 {
    margin: 0 0 0.3rem;
    font-size: 1rem;
  }
  .bubble {
    max-width: 80%;
    align-self: flex-start;
    background: var(--surface-2);
    border-radius: var(--radius);
    padding: 0.35rem 0.6rem;
  }
  .bubble.out {
    align-self: flex-end;
    background: color-mix(in srgb, var(--accent) 18%, var(--surface));
  }
  .bubble.hit {
    outline: 2px solid var(--warn);
  }
  .body {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .time {
    text-align: end;
  }
  .attachment {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
    align-items: center;
    margin-block: 0.2rem;
  }
  .attachment img {
    max-width: 100%;
    max-height: 16rem;
    border-radius: var(--radius);
  }
  .small-btn {
    font-size: 0.85em;
    padding: 0.1em 0.5em;
  }
  mark {
    background: color-mix(in srgb, var(--warn) 45%, transparent);
    color: inherit;
  }
</style>
