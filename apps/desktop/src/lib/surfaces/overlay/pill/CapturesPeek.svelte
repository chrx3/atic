<script lang="ts">
  /**
   * Vistazo de Capturas: las últimas, para llevarlas a otro lado.
   *
   * Clic copia la imagen; arrastrar la suelta como archivo en otra app (el
   * mismo arrastre nativo que las imágenes del historial). Lo demás —anotar,
   * OCR, borrar— queda en la herramienta completa.
   */
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { captures } from "$domain/captures.svelte";
  import { t } from "$domain/i18n.svelte";
  import { toastError } from "$domain/toasts.svelte";
  import { startFileDrag } from "$ipc/clipboard";
  import { setOverlayItemDrag } from "$ipc/overlay";
  import { surfaces } from "$surfaces/overlay/surfaces.svelte";
  import type { CaptureItem } from "$core/types";

  let { ondone, onnew }: { ondone: () => void; onnew: () => void } = $props();

  const LAST = 3;
  const DRAG_THRESHOLD = 4;
  /** Cuánto se ve el «Copiada» antes de cerrar. */
  const COPIED_MS = 650;

  const items = $derived(captures.items.slice(0, LAST));
  let copied = $state<string | null>(null);
  let press: { item: CaptureItem; x: number; y: number } | null = null;

  async function copy(item: CaptureItem): Promise<void> {
    try {
      await captures.copy(item.path);
      copied = item.id;
      window.setTimeout(ondone, COPIED_MS);
    } catch (error) {
      toastError(error);
    }
  }

  function onDown(event: PointerEvent, item: CaptureItem): void {
    if (event.button !== 0) return;
    // Sin esto el arrastre nativo arranca también el fantasma de imagen del webview.
    event.preventDefault();
    press = { item, x: event.clientX, y: event.clientY };
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
    void drag(p.item);
  }

  function onUp(): void {
    const p = press;
    cleanup();
    if (p) void copy(p.item);
  }

  function cleanup(): void {
    press = null;
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", onUp);
    window.removeEventListener("pointercancel", onUp);
  }

  async function drag(item: CaptureItem): Promise<void> {
    try {
      await setOverlayItemDrag(true).catch(() => {});
      await surfaces.recoverHits().catch(() => {});
      surfaces.dragging = false;
      await startFileDrag([item.path]);
    } catch (error) {
      if (!/cancel|abort|dismiss|interrupted|Dropped|Cancelled/i.test(String(error))) {
        toastError(error);
      }
    } finally {
      await setOverlayItemDrag(false).catch(() => {});
      await surfaces.recoverHits().catch(() => {});
      ondone();
    }
  }

  $effect(() => () => cleanup());
</script>

<div class="kp">
  {#if items.length === 0}
    <p class="kp-empty">{t("pill.peek.capturesEmpty")}</p>
  {:else}
    <ul class="kp-grid">
      {#each items as item (item.id)}
        <li>
          <button
            type="button"
            class="kp-shot"
            class:is-copied={copied === item.id}
            aria-label={t("pill.peek.captureCopy", { label: item.label })}
            onpointerdown={(e) => onDown(e, item)}
            onkeydown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                void copy(item);
              }
            }}
          >
            <img src={convertFileSrc(item.path)} alt="" draggable="false" />
            <span class="kp-label">
              {copied === item.id ? t("pill.peek.copied") : item.label}
            </span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}

  <button type="button" class="kp-open" onclick={onnew}
    >{t("pill.peek.capturesNew")}</button
  >
</div>

<style>
  .kp {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  .kp-empty {
    margin: 0;
    color: var(--muted);
  }

  .kp-grid {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 0.4rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .kp-shot {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    width: 100%;
    border: 0;
    padding: 0;
    background: none;
    color: var(--faint);
    font: inherit;
    font-size: 0.6875rem;
    cursor: grab;
  }

  .kp-shot img {
    width: 100%;
    aspect-ratio: 4 / 3;
    border-radius: 8px;
    box-shadow: inset 0 0 0 1px color-mix(in sRGB, var(--text) 12%, transparent);
    object-fit: cover;
    transition: transform var(--duration-quick) var(--ease-smooth-out);
  }

  .kp-shot:hover img {
    transform: scale(1.04);
  }

  .kp-shot:focus-visible {
    outline: none;
  }

  .kp-shot:focus-visible img {
    box-shadow: var(--rb-focus);
  }

  .kp-shot.is-copied .kp-label {
    color: var(--ok);
  }

  .kp-label {
    overflow: hidden;
    font-variant-numeric: tabular-nums;
    text-align: center;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .kp-open {
    align-self: flex-start;
    border: 0;
    padding: 0;
    background: none;
    color: var(--faint);
    font-size: 0.6875rem;
    cursor: pointer;
  }

  .kp-open:hover {
    color: var(--text);
  }

  .kp-open:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }
</style>
