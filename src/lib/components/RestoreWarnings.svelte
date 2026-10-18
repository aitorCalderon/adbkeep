<script lang="ts">
  import { _ } from "svelte-i18n";
  import type { PreviewApp, Reason } from "$lib/bindings";
  import { restore } from "$lib/restore.svelte";

  let { apps }: { apps: PreviewApp[] } = $props();

  const CHECKS = ["google", "battery", "unlocked", "dontUse"];

  /** Reasons of the chosen apps grouped by kind, with the packages. */
  const groups = $derived.by(() => {
    const map = new Map<string, { reason: Reason; packages: string[] }>();
    for (const a of apps) {
      const c = restore.choices[a.meta.package];
      if (!c || !(c.app || c.data.length || c.permissions) && a.compat.status !== "blocked") continue;
      for (const r of a.compat.reasons) {
        const g = map.get(r.kind) ?? { reason: r, packages: [] };
        g.packages.push(a.meta.package);
        map.set(r.kind, g);
      }
    }
    return [...map.values()];
  });
</script>

<section>
  <h3>{$_("restore.checklist.title")}</h3>
  {#each CHECKS as key}
    <label class="check"><input type="checkbox" bind:checked={restore.checklist[key]} /> {$_(`restore.checklist.${key}`)}</label>
  {/each}
</section>

<section>
  <h3>{$_("restore.warningsTitle")}</h3>
  {#if groups.length === 0}
    <p class="muted">{$_("restore.noWarnings")}</p>
  {:else}
    <ul>
      {#each groups as g}
        <li>
          {g.reason.kind === "roleTaken" ? $_(`restore.reason.roleTaken`, { values: { role: $_(`restore.roleNames.${g.reason.role.split(".").pop()}`), holder: g.reason.holder } }) : $_(`restore.reason.${g.reason.kind}`)}
          <div class="mono muted small">{g.packages.join(", ")}</div>
        </li>
      {/each}
    </ul>
  {/if}
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
  .check {
    display: block;
    margin-block: 0.25rem;
  }
  .small {
    font-size: 0.85em;
  }
  li {
    margin-block-end: 0.4rem;
  }
</style>
