<script lang="ts">
  /**
   * Historial: clic = pegar; arrastrar = OLE.
   * - Imagen: archivo (HDROP) vía tauri-plugin-drag.
   * - Texto: CF_UNICODETEXT nativo (un .txt en HDROP inserta la ruta en Cursor).
   */
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { startDrag } from "@crabnebula/tauri-plugin-drag";
  import Icon from "$ui/Icon.svelte";
  import { List, Search, Star, X } from "$lib/icons";
  import type { ClipboardItem } from "$lib/types";
  import { clipboardItemMatches } from "$lib/clipboardSearch";
  import {
    clipboardDragPath,
    deleteClipboardItem,
    pasteClipboardItem,
    pinClipboardItem,
    startClipboardTextDrag,
    tryClipboardDropOnAgents,
  } from "$lib/api";
  import { setOverlayItemDrag } from "$ipc/overlay";
  import { surfaces } from "$surfaces/overlay/surfaces.svelte";

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
  let draggingId = $state<string | null>(null);
  let query = $state("");
  let favoritesOnly = $state(false);
  let press: {
    id: string;
    x: number;
    y: number;
    item: ClipboardItem;
    /** Prefetch para no await-ear antes de DoDragDrop. */
    path: string | null;
    pathReady: boolean;
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

  /** Ventana virtual (~fila fija): evita montar hasta 100 thumbs a la vez. */
  const ROW_H = $derived(compact ? 44 : 52);
  const ROW_GAP = 5; // 0.3rem
  const OVERSCAN = 4;
  let listEl = $state<HTMLElement | null>(null);
  let scrollTop = $state(0);
  let viewportH = $state(320);

  const windowed = $derived.by(() => {
    const stride = ROW_H + ROW_GAP;
    const total = visibleItems.length;
    if (total === 0) {
      return { start: 0, end: 0, topPad: 0, bottomPad: 0, slice: [] as typeof visibleItems };
    }
    const start = Math.max(0, Math.floor(scrollTop / stride) - OVERSCAN);
    const count = Math.ceil(viewportH / stride) + OVERSCAN * 2;
    const end = Math.min(total, start + count);
    return {
      start,
      end,
      topPad: start * stride,
      bottomPad: Math.max(0, (total - end) * stride),
      slice: visibleItems.slice(start, end),
    };
  });

  function onListScroll(e: Event) {
    scrollTop = (e.currentTarget as HTMLElement).scrollTop;
  }

  $effect(() => {
    const el = listEl;
    if (!el) return;
    const measure = () => {
      viewportH = el.clientHeight || 320;
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  });

  // Al filtrar, volver arriba para no mirar una ventana vacía.
  $effect(() => {
    void query;
    void favoritesOnly;
    if (listEl) {
      listEl.scrollTop = 0;
      scrollTop = 0;
    }
  });

  function report(error: unknown) {
    onError?.(String(error));
  }

  async function paste(item: ClipboardItem) {
    if (busyId || draggingId) return;
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

  function onItemDown(event: PointerEvent, item: ClipboardItem) {
    if (event.button !== 0 || busyId || draggingId) return;
    const target = event.target as HTMLElement;
    if (target.closest(".clip-actions, .clip-icon-btn")) return;
    const seedPath =
      item.kind === "image" && item.imagePath ? item.imagePath : null;
    press = {
      id: item.id,
      x: event.clientX,
      y: event.clientY,
      item,
      path: seedPath,
      pathReady: item.kind === "text" || !!seedPath,
    };
    didDrag = false;
    window.addEventListener("pointermove", onItemMove);
    window.addEventListener("pointerup", onItemUp);
    window.addEventListener("pointercancel", onItemUp);
    // Prefetch path de imagen mientras el botón sigue abajo.
    if (item.kind === "image" && !seedPath) {
      const id = item.id;
      void clipboardDragPath(id)
        .then((path) => {
          if (press?.id === id) {
            press.path = path;
            press.pathReady = true;
          }
        })
        .catch(() => {
          if (press?.id === id) press.pathReady = true;
        });
    }
  }

  function onItemMove(event: PointerEvent) {
    if (!press || didDrag) return;
    if (Math.hypot(event.clientX - press.x, event.clientY - press.y) < DRAG_THRESHOLD) {
      return;
    }
    didDrag = true;
    const snapshot = press;
    cleanupPress();
    // OLE must start while the button is still down — no await before startDrag.
    void beginOleDrag(snapshot.item, snapshot.path);
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

  async function beginOleDrag(item: ClipboardItem, prefetched: string | null) {
    draggingId = item.id;
    try {
      await setOverlayItemDrag(true).catch(() => {});
      // Conserva hit-rect de agentes; OLE a otras apps sigue con passthrough.
      await surfaces.recoverHits().catch(() => {});
      surfaces.dragging = false;
      if (item.kind === "text") {
        // Rust cancela OLE si soltás sobre agentes e inserta en el composer.
        await startClipboardTextDrag(item.id);
        return;
      }
      let path = prefetched;
      if (!path) {
        path = await clipboardDragPath(item.id);
      }
      if (!path) {
        report("No se pudo preparar el arrastre");
        return;
      }
      await startDrag({ item: [path], icon: path, mode: "copy" });
      // Imagen: si el cursor quedó sobre agentes, insertar (sin depender del HTML5 drop).
      await tryClipboardDropOnAgents(item.id).catch(() => false);
    } catch (error) {
      const msg = String(error);
      if (/cancel|abort|dismiss|interrupted|Dropped|Cancelled/i.test(msg)) {
        /* cancel / normal end */
      } else {
        report(error);
      }
    } finally {
      draggingId = null;
      await setOverlayItemDrag(false).catch(() => {});
      await surfaces.recoverHits().catch(() => {});
    }
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
      // Siempre `delete_clipboard_item`: las capturas viven en otra carpeta
      // y el PNG no es del historial. `deleteCapture` borraba el archivo, el
      // ítem seguía en `history.json` y el reintento explotaba con os error 2.
      await deleteClipboardItem(item.id);
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
        <Icon icon={Search} size={14} />
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
          <Icon icon={List} size={14} />
        </button>
        <button
          type="button"
          class="clip-filter-btn"
          class:is-on={favoritesOnly}
          onclick={() => (favoritesOnly = true)}
          aria-label="Solo favoritos"
          title="Favoritos"
        >
          <Icon
            icon={Star}
            size={14}
            fill={favoritesOnly ? "currentColor" : "none"}
          />
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
    <ul
      class="clip-items"
      role="listbox"
      aria-label="Historial del portapapeles"
      bind:this={listEl}
      onscroll={onListScroll}
    >
      {#if windowed.topPad > 0}
        <li class="clip-pad" style:height="{windowed.topPad}px" aria-hidden="true"></li>
      {/if}
      {#each windowed.slice as item (item.id)}
        <li class="clip-row">
          <div
            class="clip-item"
            class:is-busy={busyId === item.id}
            class:is-dragging={draggingId === item.id}
            role="option"
            aria-selected={busyId === item.id || draggingId === item.id}
            tabindex="0"
            title="Clic: pegar · Arrastra a otra app o al composer"
            onpointerdown={(e) => onItemDown(e, item)}
            onkeydown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                void paste(item);
              }
            }}
          >
            <span class="clip-thumb" aria-hidden="true">
              {#if item.kind === "image" && item.imagePath}
                <img
                  src={convertFileSrc(item.imagePath)}
                  alt=""
                  draggable="false"
                  loading="lazy"
                  decoding="async"
                />
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
              <Icon
                icon={Star}
                size={14}
                fill={item.pinned ? "currentColor" : "none"}
              />
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
              <Icon icon={X} size={14} />
            </button>
          </div>
        </li>
      {/each}
      {#if windowed.bottomPad > 0}
        <li class="clip-pad" style:height="{windowed.bottomPad}px" aria-hidden="true"></li>
      {/if}
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
    gap: 0;
    margin: 0;
    padding: 0;
    overflow: auto;
    list-style: none;
  }

  .clip-pad {
    flex: none;
    margin: 0;
    padding: 0;
    list-style: none;
    pointer-events: none;
  }

  .clip-items > .clip-row {
    display: flex;
    flex: none;
    align-items: stretch;
    gap: 0.2rem;
    height: 52px;
    min-height: 52px;
    margin: 0 0 5px;
    overflow: hidden;
  }

  .is-compact .clip-items > .clip-row {
    height: 44px;
    min-height: 44px;
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
    touch-action: none;
    user-select: none;
  }
  .clip-item:active,
  .clip-item.is-dragging {
    cursor: grabbing;
  }
  .clip-item:hover {
    border-color: color-mix(in srgb, var(--rb-accent) 35%, transparent);
    background: color-mix(in srgb, var(--rb-accent) 10%, transparent);
  }
  .clip-item.is-busy {
    opacity: 0.65;
  }
  .clip-item.is-dragging {
    opacity: 0.72;
    border-color: color-mix(in srgb, var(--rb-accent) 45%, transparent);
  }
  .clip-item:focus-visible {
    outline: none;
    box-shadow: inset 0 0 0 1.5px color-mix(in srgb, var(--rb-accent) 70%, transparent);
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
  .clip-icon-btn :global(svg) {
    pointer-events: none;
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
