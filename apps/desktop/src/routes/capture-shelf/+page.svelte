<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { fly } from "svelte/transition";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { startDrag } from "@crabnebula/tauri-plugin-drag";
  import type { CaptureItem } from "$lib/types";
  import { onScreenshotCreated, activateCapture, ocrCaptureAndCopy } from "$lib/api";

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

  function clearTimer() {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
  }

  function hide() {
    clearTimer();
    current = null;
    getCurrentWindow().hide();
  }

  function scheduleDismiss() {
    clearTimer();
    timer = setTimeout(() => {
      if (!busy) hide();
    }, DISMISS_MS);
  }

  function show(item: CaptureItem) {
    current = item;
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
    // Una vez arrastrada, la notificación se va.
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

  async function doOcr(event: MouseEvent) {
    event.stopPropagation();
    if (!current || ocrBusy) return;
    ocrBusy = true;
    clearTimer();
    try {
      const text = await ocrCaptureAndCopy(current.path);
      const preview = text.length > 60 ? `${text.slice(0, 60)}…` : text;
      console.info("OCR copiado:", preview);
    } catch (error) {
      console.error("OCR falló", error);
    } finally {
      ocrBusy = false;
      hide();
    }
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
    // Clic sin arrastre → abrir preview / ubicación (según config).
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
    <div class="thumb" transition:fly={{ x: 40, y: 40, duration: 220 }}>
      <button class="grab" onmousedown={onDown} title="Clic: abrir · Arrastra: sacar">
        <img src={src} alt="captura" draggable="false" />
      </button>
      <button
        type="button"
        class="ocr"
        disabled={ocrBusy}
        title="Extraer texto (OCR)"
        onclick={(e) => void doOcr(e)}
      >
        {ocrBusy ? "…" : "Texto"}
      </button>
      <div class="name">{current.label || current.id}</div>
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

  .thumb {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px;
    width: fit-content;
  }

  .grab {
    display: block;
    flex: none;
    padding: 0;
    border: 0;
    background: none;
    cursor: grab;
  }
  .grab:active {
    cursor: grabbing;
  }
  .ocr {
    flex: none;
    border: 0;
    border-radius: 6px;
    padding: 4px 8px;
    background: rgba(255, 255, 255, 0.92);
    color: #111;
    font: 600 11px/1.2 system-ui, sans-serif;
    cursor: pointer;
  }
  .ocr:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .grab img {
    display: block;
    max-width: 96px;
    max-height: 64px;
    width: auto;
    height: auto;
    border-radius: 6px;
    box-shadow: 0 4px 14px rgba(0, 0, 0, 0.5);
  }

  .name {
    max-width: 72px;
    font: 600 12px/1.2 system-ui, sans-serif;
    color: #fff;
    text-shadow: 0 1px 3px rgba(0, 0, 0, 0.9);
    letter-spacing: 0.02em;
  }
</style>
