<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { fly } from "svelte/transition";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { startDrag } from "@crabnebula/tauri-plugin-drag";
  import { MOTION, ms } from "$lib/motion";
  import type { CaptureItem } from "$lib/types";
  import {
    onScreenshotCreated,
    activateCapture,
    ocrCaptureAndCopy,
    openDataDir,
  } from "$lib/api";

  const DISMISS_MS = 6000;
  const DRAG_THRESHOLD = 5;

  let current = $state<CaptureItem | null>(null);
  let src = $state("");
  let timer: ReturnType<typeof setTimeout> | null = null;
  let unlisten: UnlistenFn[] = [];

  let down: { x: number; y: number } | null = null;
  let dragged = false;
  let busy = $state(false);
  let ocrBusy = $state(false);
  /** Resultado del OCR: sin esto solo iba a la consola y nadie lo veía. */
  let note = $state<string | null>(null);
  /** Con el puntero encima, la tarjeta no se auto-descarta. */
  let hovering = false;

  function clearTimer() {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
  }

  function hide() {
    clearTimer();
    current = null;
    note = null;
    getCurrentWindow().hide();
  }

  function scheduleDismiss() {
    clearTimer();
    timer = setTimeout(() => {
      // Reintentar en vez de descartar: la tarjeta no debe desaparecer bajo el
      // puntero justo cuando el usuario va a arrastrarla.
      if (busy || ocrBusy || hovering) {
        scheduleDismiss();
        return;
      }
      hide();
    }, DISMISS_MS);
  }

  function onEnter() {
    hovering = true;
  }

  function onLeave() {
    hovering = false;
  }

  function show(item: CaptureItem) {
    current = item;
    note = null;
    src = `${convertFileSrc(item.path)}?t=${Date.now()}`;
    scheduleDismiss();
  }

  async function doDrag() {
    if (!current) return;
    busy = true;
    clearTimer();
    try {
      await startDrag({ item: [current.path], icon: current.path });
    } catch (error) {
      console.error("arrastre falló", error);
    }
    busy = false;
    hide();
  }

  async function doActivate() {
    if (!current) return;
    busy = true;
    try {
      await activateCapture(current.path);
    } catch (error) {
      console.error("abrir captura falló", error);
    }
    busy = false;
    hide();
  }

  /**
   * El resultado se enseña en la tarjeta antes de cerrarla. Antes iba solo a
   * la consola: si el OCR fallaba, la tarjeta se desvanecía sin decir nada.
   */
  async function doOcr(event: MouseEvent) {
    event.stopPropagation();
    if (!current || ocrBusy) return;
    ocrBusy = true;
    clearTimer();
    try {
      const text = await ocrCaptureAndCopy(current.path);
      note = text.trim()
        ? "Texto copiado al portapapeles"
        : "No se encontró texto en la captura";
    } catch (error) {
      console.error("OCR falló", error);
      note = "No se pudo extraer el texto";
    } finally {
      ocrBusy = false;
      // Deja leer el resultado en vez de cerrar de golpe.
      timer = setTimeout(hide, 2200);
    }
  }

  function openFolder(event: MouseEvent) {
    event.stopPropagation();
    clearTimer();
    void openDataDir("captures").catch(console.warn);
  }

  function cleanupPress() {
    down = null;
    window.removeEventListener("mousemove", onMove);
    window.removeEventListener("mouseup", onUp);
  }

  function onMove(e: MouseEvent) {
    if (!down || dragged) return;
    if (Math.hypot(e.clientX - down.x, e.clientY - down.y) > DRAG_THRESHOLD) {
      dragged = true;
      cleanupPress();
      doDrag();
    }
  }

  function onUp() {
    const wasClick = down !== null && !dragged;
    cleanupPress();
    if (wasClick) doActivate();
  }

  function onDown(e: MouseEvent) {
    if (e.button !== 0) return;
    down = { x: e.clientX, y: e.clientY };
    dragged = false;
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }

  onMount(async () => {
    unlisten.push(await onScreenshotCreated((item) => show(item)));
  });

  onDestroy(() => {
    for (const off of unlisten) off();
    clearTimer();
    cleanupPress();
  });
</script>

{#if current}
  {#key current.id}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="card"
      transition:fly={{ x: 40, y: 40, duration: ms(MOTION.fast) }}
      onmouseenter={onEnter}
      onmouseleave={onLeave}
    >
      <button
        class="grab"
        onmousedown={onDown}
        aria-label="Abrir captura {current.label || current.id}"
        title="Clic: abrir · Arrastra: sacar"
      >
        <img src={src} alt="" draggable="false" />
      </button>
      <div class="side">
        <div class="actions">
          <button
            type="button"
            class="chip"
            disabled={ocrBusy}
            title="Extraer texto (OCR)"
            onclick={(e) => void doOcr(e)}
          >
            {ocrBusy ? "…" : "Texto"}
          </button>
          <button
            type="button"
            class="chip"
            title="Abrir carpeta de capturas"
            onclick={openFolder}
          >
            Carpeta
          </button>
        </div>
        {#if note}
          <p class="note" role="status" aria-live="polite">{note}</p>
        {:else}
          <p class="name">{current.label || current.id}</p>
        {/if}
      </div>
    </div>
  {/key}
{/if}

<style>
  :global(html),
  :global(body) {
    margin: 0;
    background: transparent;
    overflow: hidden;
  }

  .card {
    display: flex;
    align-items: center;
    gap: 10px;
    box-sizing: border-box;
    width: 100%;
    max-width: 100%;
    height: 100%;
    padding: 8px 10px;
    overflow: hidden;
  }

  .grab {
    display: block;
    flex: 0 0 auto;
    padding: 0;
    border: 0;
    background: none;
    cursor: grab;
  }
  .grab:active {
    cursor: grabbing;
  }
  .grab img {
    display: block;
    width: 96px;
    height: 64px;
    border-radius: var(--rb-radius-sm);
    object-fit: cover;
    background: var(--rb-bg1);
    box-shadow: 0 4px 14px rgba(0, 0, 0, 0.45);
  }

  .side {
    display: flex;
    min-width: 0;
    flex: 1 1 auto;
    flex-direction: column;
    gap: 6px;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  /* Misma piel que la pill: superficie del tema, no blanco fijo. Era la única
     ventana que ignoraba claro/oscuro. */
  .chip {
    flex: 0 0 auto;
    border: 0;
    border-radius: var(--rb-radius-xs);
    padding: 4px 8px;
    background: var(--rb-surface);
    color: var(--rb-text);
    font-family: var(--rb-font);
    font-size: 0.6875rem;
    font-weight: 600;
    line-height: 1.2;
    cursor: pointer;
  }
  .chip:hover:not(:disabled) {
    background: var(--rb-surface-2);
  }
  .chip:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }
  .chip:disabled {
    opacity: 0.6;
    cursor: default;
  }

  .grab:focus-visible {
    outline: none;
    border-radius: var(--rb-radius-sm);
    box-shadow: var(--rb-focus);
  }

  .name,
  .note {
    margin: 0;
    overflow: hidden;
    max-width: 100%;
    padding: 2px 6px;
    border-radius: var(--rb-radius-xs);
    background: color-mix(in srgb, var(--rb-surface) 88%, transparent);
    color: var(--rb-text);
    font-family: var(--rb-font);
    font-size: 0.6875rem;
    font-weight: 600;
    line-height: 1.25;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .note {
    color: var(--rb-ok);
  }
</style>
