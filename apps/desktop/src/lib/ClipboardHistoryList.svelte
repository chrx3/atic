<script lang="ts">
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { startDrag } from "@crabnebula/tauri-plugin-drag";
  import List from "reicon-svelte/icons/List.svelte";
  import Search from "reicon-svelte/icons/Search.svelte";
  import Star from "reicon-svelte/icons/Star.svelte";
  import X from "reicon-svelte/icons/X.svelte";
  import type { ClipboardItem } from "$lib/types";
  import { clipboardItemMatches } from "$lib/clipboardSearch";
  import {
    deleteCapture,
    deleteClipboardItem,
    pasteClipboardItem,
    pinClipboardItem,
  } from "$lib/api";

  let {
    items = [],
    loading = false,
    compact = false,
    onRefresh,
    onPasteStart,
    onPasted,
    onError,
  }: {
    items?: ClipboardItem[];
    loading?: boolean;
    compact?: boolean;
    onRefresh: () => void | Promise<void>;
    onPasteStart?: () => void;
    onPasted?: () => void;
    onError?: (message: string) => void;
  } = $props();

  const DRAG_THRESHOLD = 6;

  let busyId = $state<string | null>(null);
  let query = $state("");
  let favoritesOnly = $state(false);
  let press: {
    id: string;
    x: number;
    y: number;
    item: ClipboardItem;
  } | null = null;
  let didDrag = false;

  const visibleItems = $derived.by(() => {
    let list = items;
    if (favoritesOnly) {
      list = list.filter((item) => item.pinned);
    }
    const q = query.trim();
    if (q) {
      list = list.filter((item) => clipboardItemMatches(item, q));
    }
    return list;
  });

  function report(error: unknown) {
    onError?.(String(error));
  }

  async function paste(item: ClipboardItem) {
    if (busyId) return;
    busyId = item.id;
    onPasteStart?.();
    try {
      await pasteClipboardItem(item.id);
      onPasted?.();
    } catch (error) {
      report(error);
    } finally {
      busyId = null;
    }
  }

  async function dragImage(item: ClipboardItem) {
    const path = item.imagePath;
    if (!path) return;
    try {
      await startDrag({ item: [path], icon: path, mode: "copy" });
    } catch (error) {
      report(error);
    }
  }

  function onItemDown(event: PointerEvent, item: ClipboardItem) {
    if (event.button !== 0 || busyId) return;
    const target = event.target as HTMLElement;
    if (target.closest(".clip-actions, .clip-icon-btn")) return;
    press = { id: item.id, x: event.clientX, y: event.clientY, item };
    didDrag = false;
    window.addEventListener("pointermove", onItemMove);
    window.addEventListener("pointerup", onItemUp);
    window.addEventListener("pointercancel", onItemUp);
  }

  function onItemMove(event: PointerEvent) {
    if (!press || didDrag) return;
    if (Math.hypot(event.clientX - press.x, event.clientY - press.y) < DRAG_THRESHOLD) {
      return;
    }
    // Imágenes: OLE file-drag (como el shelf). Texto: HTML5 text/plain.
    if (press.item.kind === "image" && press.item.imagePath) {
      didDrag = true;
      const item = press.item;
      cleanupPress();
      void dragImage(item);
    }
  }

  function onItemUp() {
    const wasClick = press !== null && !didDrag;
    const item = press?.item;
    cleanupPress();
    if (wasClick && item) void paste(item);
  }

  function cleanupPress() {
    press = null;
    window.removeEventListener("pointermove", onItemMove);
    window.removeEventListener("pointerup", onItemUp);
    window.removeEventListener("pointercancel", onItemUp);
  }

  function onTextDragStart(event: DragEvent, item: ClipboardItem) {
    const text = item.text ?? item.preview ?? "";
    if (!text) {
      event.preventDefault();
      return;
    }
    didDrag = true;
    cleanupPress();
    event.dataTransfer?.setData("text/plain", text);
    event.dataTransfer?.setData("text", text);
    if (event.dataTransfer) event.dataTransfer.effectAllowed = "copy";
  }

  async function togglePin(item: ClipboardItem, event: MouseEvent) {
    event.stopPropagation();
    event.preventDefault();
    try {
      await pinClipboardItem(item.id, !item.pinned);
      await onRefresh();
    } catch (error) {
      report(error);
    }
  }

  async function remove(item: ClipboardItem, event: MouseEvent) {
    event.stopPropagation();
    event.preventDefault();
    try {
      if (item.id.startsWith("capture-")) {
        if (!item.imagePath) return;
        await deleteCapture(item.imagePath);
      } else {
        await deleteClipboardItem(item.id);
      }
      await onRefresh();
    } catch (error) {
      report(error);
    }
  }

</script>

<div class="clip-list" class:is-compact={compact}>
  <div class="clip-toolbar">
    <label class="clip-search-wrap">
      <span class="clip-search-icon" aria-hidden="true">
        <Search size={14} />
      </span>
      <input
        class="clip-search"
        type="search"
        placeholder="Buscar…"
        autocomplete="off"
        spellcheck="false"
        bind:value={query}
        aria-label="Buscar en el historial"
      />
    </label>
    <div class="clip-toolbar-row">
      <span class="clip-count">
        {#if loading}
          Cargando…
        {:else if query.trim() || favoritesOnly}
          {visibleItems.length}/{items.length}
        {:else}
          {items.length} ítems
        {/if}
      </span>
      <div class="clip-filters" role="group" aria-label="Filtrar historial">
        <button
          type="button"
          class="clip-filter-btn"
          class:is-on={!favoritesOnly}
          onclick={() => (favoritesOnly = false)}
          aria-label="Mostrar todos"
          title="Todos"
        >
          <List size={14} />
        </button>
        <button
          type="button"
          class="clip-filter-btn"
          class:is-on={favoritesOnly}
          onclick={() => (favoritesOnly = true)}
          aria-label="Solo favoritos"
          title="Favoritos"
        >
          <Star size={14} weight={favoritesOnly ? "Filled" : "Outline"} />
        </button>
      </div>
    </div>
  </div>

  {#if !loading && items.length === 0}
    <p class="clip-empty">Copia texto o una imagen para empezar el historial.</p>
  {:else if !loading && visibleItems.length === 0}
    <p class="clip-empty">
      {#if favoritesOnly && !query.trim()}
        No hay favoritos. Marca ítems con la estrella.
      {:else}
        Sin coincidencias.
      {/if}
    </p>
  {:else}
    <ul class="clip-items" role="listbox" aria-label="Historial del portapapeles">
      {#each visibleItems as item (item.id)}
        <li>
          <!-- Texto: draggable HTML5 para soltar text/plain. Imagen: startDrag por umbral. -->
          <div
            class="clip-item"
            class:is-busy={busyId === item.id}
            class:is-text={item.kind === "text"}
            role="option"
            aria-selected="false"
            tabindex="0"
            title={item.kind === "text"
              ? "Clic: pegar · Arrastra: soltar texto"
              : "Clic: pegar · Arrastra: soltar imagen"}
            draggable={item.kind === "text"}
            onpointerdown={(e) => onItemDown(e, item)}
            ondragstart={(e) => {
              if (item.kind === "text") onTextDragStart(e, item);
              else e.preventDefault();
            }}
            onkeydown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                void paste(item);
              }
            }}
          >
            <span class="clip-thumb" aria-hidden="true">
              {#if item.kind === "image" && item.imagePath}
                <img src={convertFileSrc(item.imagePath)} alt="" draggable="false" />
              {:else}
                <span class="clip-text-icon">Aa</span>
              {/if}
            </span>
            <span class="clip-body">
              <span class="clip-preview">{item.preview || "(vacío)"}</span>
              <span class="clip-meta">
                {item.kind === "image" ? "Imagen" : "Texto"}
                {#if item.pinned}
                  · Fav
                {/if}
                {#if item.source === "capture"}
                  · Captura
                {/if}
              </span>
            </span>
          </div>
          <div class="clip-actions">
            <button
              type="button"
              class="clip-icon-btn"
              class:is-on={item.pinned}
              onpointerdown={(e) => e.stopPropagation()}
              onclick={(e) => void togglePin(item, e)}
              aria-label={item.pinned ? "Quitar de favoritos" : "Marcar favorito"}
              title={item.pinned ? "Quitar de favoritos" : "Marcar favorito"}
            >
              <Star size={14} weight={item.pinned ? "Filled" : "Outline"} />
            </button>
            <button
              type="button"
              class="clip-icon-btn"
              onpointerdown={(e) => {
                e.stopPropagation();
                e.preventDefault();
              }}
              onclick={(e) => {
                e.stopPropagation();
                e.preventDefault();
                void remove(item, e);
              }}
              aria-label="Eliminar"
              title="Eliminar"
            >
              <X size={14} />
            </button>
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .clip-list {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    gap: 0.45rem;
  }

  .clip-toolbar {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    padding: 0 0.15rem;
  }

  .clip-toolbar-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.4rem;
  }

  .clip-search-wrap {
    position: relative;
    display: block;
    width: 100%;
  }

  .clip-search-icon {
    position: absolute;
    top: 50%;
    left: 0.4rem;
    display: grid;
    place-items: center;
    color: var(--rb-muted);
    pointer-events: none;
    transform: translateY(-50%);
  }

  .clip-search {
    width: 100%;
    box-sizing: border-box;
    border: 0;
    border-radius: 8px;
    margin: 0;
    padding: 0.28rem 0.45rem 0.28rem 1.55rem;
    background: color-mix(in srgb, var(--rb-text) 4%, transparent);
    color: var(--rb-text);
    font-size: 0.6875rem;
    font-weight: 500;
    outline: none;
  }
  .clip-search::placeholder {
    color: var(--rb-muted);
  }
  .clip-search:focus {
    border-color: color-mix(in srgb, var(--rb-accent) 55%, var(--rb-border));
  }

  .clip-count {
    flex: 1;
    min-width: 0;
    color: var(--rb-muted);
    font-size: 0.6875rem;
    font-weight: 600;
  }

  .clip-filters {
    display: flex;
    flex-shrink: 0;
    gap: 0.15rem;
  }

  .clip-filter-btn {
    display: grid;
    min-width: 1.55rem;
    height: 1.35rem;
    place-items: center;
    border: 0;
    border-radius: 6px;
    margin: 0;
    padding: 0;
    background: transparent;
    color: var(--rb-muted);
    cursor: pointer;
    line-height: 1;
  }
  .clip-filter-btn:hover {
    background: color-mix(in srgb, var(--rb-text) 8%, transparent);
    color: var(--rb-text);
  }
  .clip-filter-btn.is-on {
    background: color-mix(in srgb, var(--rb-accent) 14%, transparent);
    color: var(--rb-accent);
  }

  .clip-empty {
    margin: 0;
    padding: 0.75rem 0.35rem;
    color: var(--rb-muted);
    font-size: 0.75rem;
    line-height: 1.4;
  }

  .clip-items {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    gap: 0.3rem;
    margin: 0;
    padding: 0;
    overflow: auto;
    list-style: none;
  }

  .clip-items > li {
    display: flex;
    align-items: stretch;
    gap: 0.2rem;
  }

  .clip-item {
    display: flex;
    min-width: 0;
    flex: 1;
    align-items: center;
    gap: 0.55rem;
    border: 1px solid transparent;
    border-radius: 10px;
    margin: 0;
    padding: 0.4rem 0.45rem;
    background: color-mix(in srgb, var(--rb-text) 5%, transparent);
    color: var(--rb-text);
    cursor: grab;
    text-align: left;
  }
  .clip-item:active {
    cursor: grabbing;
  }
  .clip-item:hover {
    border-color: color-mix(in srgb, var(--rb-accent) 35%, transparent);
    background: color-mix(in srgb, var(--rb-accent) 10%, transparent);
  }
  .clip-item.is-busy {
    opacity: 0.65;
  }
  .clip-item.is-text {
    cursor: grab;
  }

  .clip-thumb {
    display: grid;
    width: 36px;
    height: 36px;
    flex-shrink: 0;
    place-items: center;
    overflow: hidden;
    border-radius: 8px;
    background: color-mix(in srgb, var(--rb-text) 8%, transparent);
    pointer-events: none;
  }
  .clip-thumb img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .clip-text-icon {
    color: var(--rb-muted);
    font-size: 0.7rem;
    font-weight: 700;
  }

  .clip-body {
    display: flex;
    min-width: 0;
    flex: 1;
    flex-direction: column;
    gap: 0.1rem;
    pointer-events: none;
  }

  .clip-preview {
    overflow: hidden;
    font-size: 0.75rem;
    font-weight: 500;
    line-height: 1.25;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .clip-meta {
    color: var(--rb-muted);
    font-size: 0.625rem;
  }

  .clip-actions {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 0.1rem;
  }

  .clip-icon-btn {
    display: grid;
    min-width: 1.55rem;
    height: 1.35rem;
    place-items: center;
    border: 0;
    border-radius: 6px;
    margin: 0;
    padding: 0;
    background: transparent;
    color: var(--rb-muted);
    cursor: pointer;
    line-height: 1;
  }
  .clip-icon-btn:hover {
    background: color-mix(in srgb, var(--rb-text) 8%, transparent);
    color: var(--rb-text);
  }
  .clip-icon-btn.is-on {
    color: var(--rb-accent);
  }

  .is-compact .clip-thumb {
    width: 30px;
    height: 30px;
  }
  .is-compact .clip-item {
    padding: 0.3rem 0.35rem;
  }
  .is-compact .clip-search {
    padding: 0.22rem 0.4rem 0.22rem 1.45rem;
  }

  @container atic-main (max-width: 36.999rem) {
    .clip-toolbar-row {
      flex-wrap: wrap;
    }

    .clip-filter-btn,
    .clip-icon-btn {
      min-width: 2rem;
      height: 2rem;
    }

    .clip-search {
      min-height: 2.25rem;
      font-size: 0.8125rem;
    }
  }
</style>
