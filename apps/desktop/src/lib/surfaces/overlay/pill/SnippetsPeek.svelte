<script lang="ts">
  /**
   * Vistazo de Textos: los últimos que editaste, a un clic de pegarlos.
   *
   * «Últimos editados» y no «más usados»: los textos no llevan la cuenta de
   * cuántas veces se pegaron. Pega igual que la cara de textos
   * (`pasteSnippet`): el overlay no roba el foco y el texto cae en la app de
   * atrás.
   */
  import { snippets } from "$domain/snippets.svelte";
  import { t } from "$domain/i18n.svelte";
  import { toastError } from "$domain/toasts.svelte";
  import { pasteSnippet } from "$ipc/snippets";
  import type { Snippet } from "$core/types";

  let { onpasted, onopen }: { onpasted: () => void; onopen: () => void } = $props();

  const LAST = 3;
  const items = $derived(
    [...snippets.items].sort((a, b) => b.updatedAtMs - a.updatedAtMs).slice(0, LAST),
  );
  let busy = $state<string | null>(null);

  function preview(item: Snippet): string {
    return item.body.replace(/\s+/g, " ").trim();
  }

  async function paste(item: Snippet): Promise<void> {
    if (busy) return;
    busy = item.id;
    try {
      await pasteSnippet(item.id);
      onpasted();
    } catch (error) {
      toastError(error);
    } finally {
      busy = null;
    }
  }
</script>

<div class="np">
  {#if items.length === 0}
    <p class="np-empty">
      {snippets.loading ? t("pill.quota.loading") : t("pill.peek.snippetsEmpty")}
    </p>
  {:else}
    <ul class="np-list">
      {#each items as item (item.id)}
        <li>
          <button
            type="button"
            class="np-item"
            disabled={busy !== null}
            aria-label={t("pill.peek.paste", { label: item.name })}
            onclick={() => void paste(item)}
          >
            <span class="np-name">{item.name}</span>
            <span class="np-body">{preview(item)}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}

  <button type="button" class="np-open" onclick={onopen}
    >{t("pill.peek.snippetsOpen")}</button
  >
</div>

<style>
  .np {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }

  .np-empty {
    margin: 0;
    color: var(--muted);
  }

  .np-list {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    margin: 0 -0.35rem;
    padding: 0;
    list-style: none;
  }

  .np-item {
    display: flex;
    flex-direction: column;
    gap: 0.05rem;
    width: 100%;
    min-width: 0;
    border: 0;
    border-radius: 10px;
    padding: 0.3rem 0.35rem;
    background: transparent;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition: background var(--duration-quick) var(--ease-smooth-out);
  }

  .np-item:hover:not(:disabled) {
    background: color-mix(in sRGB, var(--text) 10%, transparent);
  }

  .np-item:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }

  .np-item:disabled {
    cursor: progress;
  }

  .np-name,
  .np-body {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .np-name {
    font-weight: 600;
  }

  .np-body {
    color: var(--faint);
    font-size: 0.6875rem;
  }

  .np-open {
    align-self: flex-start;
    border: 0;
    padding: 0;
    background: none;
    color: var(--faint);
    font-size: 0.6875rem;
    cursor: pointer;
  }

  .np-open:hover {
    color: var(--text);
  }

  .np-open:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }
</style>
