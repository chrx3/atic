<script lang="ts">
  /**
   * Vistazo de Clipboard: lo último que copiaste, a un clic de pegarlo.
   *
   * Pega igual que la cara del historial (`pasteClipboardItem`): el overlay no
   * roba el foco, así que el texto cae en la app de atrás. Arrastrar lo lleva a
   * otra app como en el historial: el texto como texto (Rust lo inserta si cae
   * sobre un agente) y la imagen como archivo. Lo demás —buscar, fijar,
   * borrar— queda en el historial completo.
   */
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { clipboard } from "$domain/clipboard.svelte";
  import { t } from "$domain/i18n.svelte";
  import { toastError } from "$domain/toasts.svelte";
  import { formatListWhen } from "$core/format";
  import {
    clipboardDragPath,
    dispatchClipboardOle,
    pasteClipboardItem,
    startClipboardTextDrag,
    startFileDrag,
    tryClipboardDropOnAgents,
  } from "$ipc/clipboard";
  import { setOverlayItemDrag } from "$ipc/overlay";
  import { surfaces } from "$surfaces/overlay/surfaces.svelte";
  import type { ClipboardItem } from "$lib/types";

  let { onpasted, onopen }: { onpasted: () => void; onopen: () => void } = $props();

  const LAST = 3;
  const DRAG_THRESHOLD = 4;
  const items = $derived(clipboard.items.slice(0, LAST));
  let busy = $state<string | null>(null);
  /** Botón abajo sobre una fila. `path`: la imagen, pedida mientras se sostiene. */
  let press: { item: ClipboardItem; x: number; y: number; path: string | null } | null =
    null;

  function label(item: ClipboardItem): string {
    if (item.kind === "image") return t("pill.peek.clipboardImage");
    return (item.text || item.preview || "").replace(/\s+/g, " ").trim();
  }

  async function paste(item: ClipboardItem): Promise<void> {
    if (busy) return;
    busy = item.id;
    try {
      await pasteClipboardItem(item.id);
      onpasted();
    } catch (error) {
      toastError(error);
    } finally {
      busy = null;
    }
  }

  function onDown(event: PointerEvent, item: ClipboardItem): void {
    if (event.button !== 0 || busy) return;
    // Sin esto el arrastre nativo arranca también una selección de texto.
    event.preventDefault();
    press = {
      item,
      x: event.clientX,
      y: event.clientY,
      path: item.kind === "image" ? (item.imagePath ?? null) : null,
    };
    // La ruta de la imagen se pide ya: el arrastre nativo no puede esperarla.
    if (item.kind === "image" && !item.imagePath) {
      const id = item.id;
      void clipboardDragPath(id)
        .then((path) => {
          if (press?.item.id === id) press.path = path;
        })
        .catch(() => {});
    }
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onUp);
  }

  function onMove(event: PointerEvent): void {
    const p = press;
    if (!p || Math.hypot(event.clientX - p.x, event.clientY - p.y) < DRAG_THRESHOLD)
      return;
    cleanup();
    // El arrastre nativo tiene que empezar con el botón todavía abajo.
    void drag(p.item, p.path);
  }

  function onUp(): void {
    const p = press;
    cleanup();
    if (p) void paste(p.item);
  }

  function cleanup(): void {
    press = null;
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", onUp);
    window.removeEventListener("pointercancel", onUp);
  }

  async function drag(item: ClipboardItem, path: string | null): Promise<void> {
    dispatchClipboardOle(true);
    try {
      await setOverlayItemDrag(true).catch(() => {});
      await surfaces.recoverHits().catch(() => {});
      surfaces.dragging = false;
      if (item.kind === "text") {
        await startClipboardTextDrag(item.id);
        return;
      }
      const file = path ?? (await clipboardDragPath(item.id));
      if (!file) return;
      await startFileDrag([file]);
      await tryClipboardDropOnAgents(item.id).catch(() => false);
    } catch (error) {
      if (!/cancel|abort|dismiss|interrupted|Dropped|Cancelled/i.test(String(error))) {
        toastError(error);
      }
    } finally {
      dispatchClipboardOle(false);
      await setOverlayItemDrag(false).catch(() => {});
      await surfaces.recoverHits().catch(() => {});
      onpasted();
    }
  }

  $effect(() => () => cleanup());
</script>

<div class="cp">
  {#if items.length === 0}
    <p class="cp-empty">
      {clipboard.loading ? t("pill.quota.loading") : t("pill.peek.clipboardEmpty")}
    </p>
  {:else}
    <ul class="cp-list">
      {#each items as item (item.id)}
        {@const text = label(item)}
        <li>
          <button
            type="button"
            class="cp-item"
            disabled={busy !== null}
            aria-label={t("pill.peek.paste", { label: text })}
            onpointerdown={(e) => onDown(e, item)}
            onkeydown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                void paste(item);
              }
            }}
          >
            {#if item.kind === "image" && item.imagePath}
              <img
                class="cp-thumb"
                src={convertFileSrc(item.imagePath)}
                alt=""
                draggable="false"
              />
            {/if}
            <span class="cp-text">{text}</span>
            <span class="cp-when">{formatListWhen(item.createdAtMs / 1000)}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}

  <button type="button" class="cp-open" onclick={onopen}>
    {t("pill.peek.openClipboard")}
  </button>
</div>

<style>
  .cp {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }

  .cp-empty {
    margin: 0;
    color: var(--muted);
  }

  .cp-list {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    margin: 0 -0.35rem;
    padding: 0;
    list-style: none;
  }

  .cp-item {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    width: 100%;
    min-width: 0;
    border: 0;
    border-radius: 10px;
    padding: 0.3rem 0.35rem;
    background: transparent;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: grab;
    transition: background var(--duration-quick) var(--ease-smooth-out);
  }

  .cp-item:hover:not(:disabled) {
    background: color-mix(in sRGB, var(--text) 10%, transparent);
  }

  .cp-item:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }

  .cp-item:disabled {
    cursor: progress;
  }

  .cp-thumb {
    flex: 0 0 auto;
    width: 2.75rem;
    height: 2rem;
    border-radius: 6px;
    object-fit: cover;
  }

  .cp-text {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .cp-when {
    flex: 0 0 auto;
    color: var(--faint);
    font-size: 0.6875rem;
  }

  .cp-open {
    align-self: flex-start;
    border: 0;
    padding: 0;
    background: none;
    color: var(--faint);
    font-size: 0.6875rem;
    cursor: pointer;
  }

  .cp-open:hover {
    color: var(--text);
  }

  .cp-open:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }
</style>
