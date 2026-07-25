<script lang="ts">
  import { tick } from "svelte";
  import type { SearchHit, SearchHitKind } from "$lib/types";
  import { searchLocal } from "$lib/api";
  import ModalShell from "$lib/ModalShell.svelte";

  let {
    open = $bindable(false),
    onSelect,
    onClose,
  }: {
    open?: boolean;
    onSelect: (hit: SearchHit) => void | Promise<void>;
    onClose: () => void;
  } = $props();

  let query = $state("");
  let results = $state<SearchHit[]>([]);
  let loading = $state(false);
  let activeIndex = $state(0);
  let inputEl = $state<HTMLInputElement | undefined>(undefined);
  let searchTimer: ReturnType<typeof setTimeout> | null = null;

  const kindLabels: Record<SearchHitKind, string> = {
    snippet: "Fragmentos",
    clipboard: "Portapapeles",
    capture: "Capturas",
    scratchpad: "Bloc",
    recording: "Reuniones",
  };

  const grouped = $derived.by(() => {
    const map = new Map<SearchHitKind, SearchHit[]>();
    for (const hit of results) {
      const list = map.get(hit.kind) ?? [];
      list.push(hit);
      map.set(hit.kind, list);
    }
    return [...map.entries()];
  });

  function flatResults(): SearchHit[] {
    return grouped.flatMap(([, items]) => items);
  }

  async function runSearch(value: string) {
    const trimmed = value.trim();
    if (!trimmed) {
      results = [];
      activeIndex = 0;
      return;
    }
    loading = true;
    try {
      results = await searchLocal(trimmed);
      activeIndex = 0;
    } catch {
      results = [];
    } finally {
      loading = false;
    }
  }

  function scheduleSearch(value: string) {
    if (searchTimer) clearTimeout(searchTimer);
    searchTimer = setTimeout(() => {
      void runSearch(value);
    }, 120);
  }

  async function choose(hit: SearchHit) {
    await onSelect(hit);
    open = false;
    onClose();
  }

  function handleKeydown(event: KeyboardEvent) {
    const flat = flatResults();
    if (event.key === "ArrowDown") {
      event.preventDefault();
      if (flat.length > 0) {
        activeIndex = (activeIndex + 1) % flat.length;
      }
      return;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      if (flat.length > 0) {
        activeIndex = (activeIndex - 1 + flat.length) % flat.length;
      }
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      const hit = flat[activeIndex];
      if (hit) void choose(hit);
    }
  }

  $effect(() => {
    if (open) {
      query = "";
      results = [];
      activeIndex = 0;
      void tick().then(() => inputEl?.focus());
    }
  });

  $effect(() => {
    if (open) scheduleSearch(query);
  });
</script>

{#if open}
  <ModalShell
    title="Buscar"
    subtitle="Fragmentos, portapapeles, capturas, bloc y reuniones"
    size="lg"
    onClose={() => {
      open = false;
      onClose();
    }}
  >
  <div class="search-box">
    <input
      bind:this={inputEl}
      class="search-input"
      type="search"
      placeholder="Escribe para buscar…"
      bind:value={query}
      onkeydown={handleKeydown}
      aria-label="Buscar en Atic"
      autocomplete="off"
      spellcheck="false"
    />
    {#if loading}
      <p class="search-hint">Buscando…</p>
    {:else if query.trim() && results.length === 0}
      <p class="search-hint">Sin resultados.</p>
    {:else}
      <div class="search-results" role="listbox" aria-label="Resultados">
        {#each grouped as [kind, items] (kind)}
          <p class="search-group">{kindLabels[kind]}</p>
          <ul class="search-list">
            {#each items as hit (hit.id + hit.kind)}
              {@const flat = flatResults()}
              {@const idx = flat.findIndex(
                (item) => item.id === hit.id && item.kind === hit.kind,
              )}
              <li>
                <button
                  type="button"
                  class="search-item"
                  class:active={idx === activeIndex}
                  role="option"
                  aria-selected={idx === activeIndex}
                  onclick={() => void choose(hit)}
                >
                  <span class="search-item-title">{hit.title}</span>
                  <span class="search-item-preview">{hit.preview}</span>
                </button>
              </li>
            {/each}
          </ul>
        {/each}
      </div>
    {/if}
  </div>
  </ModalShell>
{/if}

<style>
  .search-box {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    min-height: 14rem;
  }

  .search-input {
    width: 100%;
    border: 0;
    border-radius: var(--rb-radius-sm);
    padding: 0.55rem 0.7rem;
    background: var(--rb-bg1);
    color: var(--rb-text);
    font-size: 0.9375rem;
    outline: none;
  }

  .search-input:focus {
    border-color: color-mix(in srgb, var(--rb-accent) 45%, var(--rb-border));
    box-shadow: var(--rb-focus);
  }

  .search-hint {
    margin: 0;
    color: var(--rb-muted);
    font-size: 0.875rem;
  }

  .search-results {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    max-height: 22rem;
    overflow: auto;
  }

  .search-group {
    margin: 0.35rem 0 0.15rem;
    color: var(--rb-muted);
    font-size: 0.6875rem;
    font-weight: 650;
    letter-spacing: 0.05em;
    text-transform: uppercase;
  }

  .search-list {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .search-item {
    display: flex;
    width: 100%;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.15rem;
    border: 1px solid transparent;
    border-radius: var(--rb-radius-sm);
    padding: 0.45rem 0.55rem;
    background: transparent;
    color: var(--rb-text);
    text-align: left;
    cursor: pointer;
  }

  .search-item:hover,
  .search-item.active {
    border-color: color-mix(in srgb, var(--rb-accent) 30%, transparent);
    background: color-mix(in srgb, var(--rb-accent) 8%, transparent);
  }

  .search-item-title {
    font-size: 0.8125rem;
    font-weight: 650;
  }

  .search-item-preview {
    color: var(--rb-muted);
    font-size: 0.75rem;
    line-height: 1.35;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
</style>
