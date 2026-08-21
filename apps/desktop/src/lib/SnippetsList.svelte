<script lang="ts">
  import Icon from "$ui/Icon.svelte";
  import { Search, Trash2 } from "$lib/icons";
  import type { Snippet as TextSnippet } from "$lib/types";
  import { emptySnippet } from "$lib/snippetsModel";
  import { deleteSnippet, pasteSnippet } from "$lib/api";

  let {
    items = [],
    loading = false,
    compact = false,
    onRefresh,
    onEdit,
    onPasteStart,
    onPasted,
    onError,
  }: {
    items?: TextSnippet[];
    loading?: boolean;
    compact?: boolean;
    onRefresh: () => void | Promise<void>;
    onEdit?: (snippet: TextSnippet) => void;
    onPasteStart?: () => void;
    onPasted?: () => void;
    onError?: (message: string) => void;
  } = $props();

  let busyId = $state<string | null>(null);
  let query = $state("");

  const visibleItems = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!q) return items;
    return items.filter((item) => {
      if (item.name.toLowerCase().includes(q)) return true;
      if (item.body.toLowerCase().includes(q)) return true;
      return item.aliases.some((alias) => alias.toLowerCase().includes(q));
    });
  });

  function report(error: unknown) {
    onError?.(String(error));
  }

  function preview(body: string): string {
    const line = body.split("\n").find((l) => l.trim()) ?? "";
    return line.length > 120 ? `${line.slice(0, 117)}…` : line;
  }

  async function paste(item: TextSnippet) {
    if (busyId) return;
    busyId = item.id;
    onPasteStart?.();
    try {
      await pasteSnippet(item.id);
      onPasted?.();
    } catch (error) {
      report(error);
    } finally {
      busyId = null;
    }
  }

  async function remove(item: TextSnippet, event: MouseEvent) {
    event.stopPropagation();
    if (busyId) return;
    busyId = item.id;
    try {
      await deleteSnippet(item.id);
      await onRefresh();
    } catch (error) {
      report(error);
    } finally {
      busyId = null;
    }
  }
</script>

<div class="snip-list" class:compact>
  <div class="snip-toolbar">
    <label class="snip-search">
      <Icon icon={Search} size={14} />
      <input
        type="search"
        placeholder="Buscar por nombre o palabra…"
        bind:value={query}
        aria-label="Buscar textos"
      />
    </label>
    {#if !compact && onEdit}
      <button type="button" class="snip-new" onclick={() => onEdit(emptySnippet())}>
        Nuevo
      </button>
    {/if}
  </div>

  {#if loading}
    <p class="snip-empty">Cargando…</p>
  {:else if visibleItems.length === 0}
    <!-- Una lista vacía es el momento en que alguien decide si vale la pena
         entender la herramienta. "Aún no hay fragmentos guardados" no explicaba
         qué es un fragmento ni para qué sirve. -->
    <p class="snip-empty">
      {query.trim()
        ? "Sin coincidencias."
        : "Guarda los textos que escribes seguido —tu firma, un saludo, una plantilla— y pégalos desde la pill sin volver a tipearlos."}
    </p>
  {:else}
    <ul class="snip-items" role="list">
      {#each visibleItems as item (item.id)}
        <li>
          <button
            type="button"
            class="snip-item"
            class:is-busy={busyId === item.id}
            onclick={() => void paste(item)}
            ondblclick={() => onEdit?.(item)}
            title={item.body}
          >
            <span class="snip-name">{item.name}</span>
            {#if item.aliases.length > 0}
              <span class="snip-aliases">{item.aliases.join(" · ")}</span>
            {/if}
            <span class="snip-preview">{preview(item.body)}</span>
          </button>
          {#if !compact && onEdit}
            <div class="snip-actions">
              <button
                type="button"
                class="snip-icon-btn"
                aria-label="Editar {item.name}"
                onclick={() => onEdit(item)}
              >
                Editar
              </button>
              <button
                type="button"
                class="snip-icon-btn is-danger"
                aria-label="Eliminar {item.name}"
                onclick={(event) => void remove(item, event)}
              >
                <Icon icon={Trash2} size={14} />
              </button>
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .snip-list {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    gap: 0.5rem;
  }

  .snip-toolbar {
    display: flex;
    align-items: center;
    gap: 0.45rem;
  }

  .snip-search {
    display: flex;
    min-width: 0;
    flex: 1;
    align-items: center;
    gap: 0.35rem;
    border: 0;
    border-radius: 0.45rem;
    padding: 0.3rem 0.45rem;
    background: var(--rb-bg0);
    color: var(--rb-muted);
  }

  .snip-search input {
    width: 100%;
    border: 0;
    background: transparent;
    color: var(--rb-text);
    font-size: 0.8125rem;
    outline: none;
  }

  .snip-new {
    flex-shrink: 0;
    border: 0;
    border-radius: 0.45rem;
    padding: 0.35rem 0.65rem;
    background: var(--rb-accent-soft);
    color: var(--rb-accent);
    font-size: 0.75rem;
    font-weight: 600;
    cursor: pointer;
  }

  .snip-items {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    gap: 0.35rem;
    margin: 0;
    padding: 0;
    list-style: none;
    overflow: auto;
  }

  .snip-items li {
    display: flex;
    align-items: stretch;
    gap: 0.35rem;
  }

  .snip-item {
    display: flex;
    min-width: 0;
    flex: 1;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.15rem;
    border: 0;
    border-radius: 0.5rem;
    padding: 0.45rem 0.55rem;
    background: var(--rb-bg0);
    color: var(--rb-text);
    text-align: left;
    cursor: pointer;
  }

  .snip-item:hover {
    border-color: color-mix(in srgb, var(--rb-accent) 35%, var(--rb-border));
  }

  .snip-item.is-busy {
    opacity: 0.6;
    pointer-events: none;
  }

  .snip-name {
    font-size: 0.8125rem;
    font-weight: 650;
  }

  .snip-aliases {
    font-size: 0.6875rem;
    color: var(--rb-accent);
  }

  .snip-preview {
    width: 100%;
    overflow: hidden;
    color: var(--rb-muted);
    font-size: 0.75rem;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .snip-actions {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .snip-icon-btn {
    border: 0;
    border-radius: 0.4rem;
    padding: 0.25rem 0.35rem;
    background: transparent;
    color: var(--rb-muted);
    font-size: 0.6875rem;
    cursor: pointer;
  }

  .snip-icon-btn.is-danger:hover {
    color: var(--rb-record);
    border-color: color-mix(in srgb, var(--rb-record) 40%, var(--rb-border));
  }

  .snip-empty {
    margin: 0.5rem 0 0;
    color: var(--rb-muted);
    font-size: 0.8125rem;
  }

  .compact .snip-toolbar {
    gap: 0.35rem;
  }

  .compact .snip-item {
    padding: 0.4rem 0.5rem;
  }

  @container atic-main (max-width: 36.999rem) {
    .snip-toolbar {
      flex-wrap: wrap;
    }

    .snip-new {
      min-height: 2.25rem;
    }

    .snip-search {
      min-height: 2.25rem;
    }

    .snip-icon-btn {
      min-height: 2rem;
      min-width: 2rem;
    }
  }
</style>
